//! Where MIDI bytes go. Only the worker thread touches sinks.
//!
//! [`AlsaOpener`] uses the kernel's documented ALSA rawmidi character devices
//! (`/dev/snd/midiC<card>D<device>`, Documentation/sound in the kernel tree):
//! opened write-only with O_NONBLOCK, so a stalled port returns EAGAIN
//! instead of blocking. Port names come from `/proc/asound/card<N>/id` and
//! `/proc/asound/card<N>/midi<D>`. No alsa-lib, no sequencer daemon.

use std::fs;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SinkError {
    /// the port is busy; try again shortly
    WouldBlock,
    /// the port went away (unplugged, gadget removed)
    Gone(String),
}

pub trait MidiSink: Send {
    /// Write some of `b`; returns how many bytes were taken.
    fn write(&mut self, b: &[u8]) -> Result<usize, SinkError>;
    fn name(&self) -> String;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortInfo {
    /// "hw:1,0"
    pub id: String,
    /// card id + port name, e.g. "Trill: nRF52840 MIDI 1"
    pub name: String,
    pub path: PathBuf,
}

impl PortInfo {
    /// Short label for small screens / config: the name (or id if unnamed).
    pub fn label(&self) -> String {
        let s = if self.name.is_empty() { self.id.clone() } else { self.name.clone() };
        s.chars().filter(|c| c.is_ascii() && !c.is_ascii_control()).take(crate::config::PORT_MAX).collect()
    }
    /// Whether a configured port name selects this port.
    pub fn matches(&self, want: &str) -> bool {
        want.is_empty() || want.eq_ignore_ascii_case("auto") || self.label() == want || self.id == want
    }
}

pub trait Opener: Send {
    fn list(&mut self) -> Vec<PortInfo>;
    fn open(&mut self, p: &PortInfo) -> Result<Box<dyn MidiSink>, String>;
}

// ---------------------------------------------------------------- ALSA rawmidi

pub struct AlsaOpener {
    pub dev_dir: PathBuf,
    pub proc_dir: PathBuf,
    /// test hook: a single explicit path (e.g. a FIFO); `ZPOD_MIDI_PATH`
    pub override_path: Option<PathBuf>,
}

impl Default for AlsaOpener {
    fn default() -> Self {
        AlsaOpener { dev_dir: "/dev/snd".into(), proc_dir: "/proc/asound".into(), override_path: std::env::var_os("ZPOD_MIDI_PATH").map(PathBuf::from) }
    }
}

fn first_line(p: &Path) -> String {
    fs::read_to_string(p).ok().and_then(|s| s.lines().next().map(|l| l.trim().to_string())).unwrap_or_default()
}

impl Opener for AlsaOpener {
    fn list(&mut self) -> Vec<PortInfo> {
        if let Some(p) = &self.override_path {
            return vec![PortInfo { id: "path".into(), name: format!("test:{}", p.file_name().and_then(|n| n.to_str()).unwrap_or("?")), path: p.clone() }];
        }
        let mut v = Vec::new();
        let Ok(rd) = fs::read_dir(&self.dev_dir) else { return v };
        for e in rd.flatten() {
            let n = e.file_name();
            let Some(n) = n.to_str() else { continue };
            let Some(rest) = n.strip_prefix("midiC") else { continue };
            let Some((c, d)) = rest.split_once('D') else { continue };
            let (Ok(c), Ok(d)) = (c.parse::<u32>(), d.parse::<u32>()) else { continue };
            let card = first_line(&self.proc_dir.join(format!("card{c}/id")));
            let port = first_line(&self.proc_dir.join(format!("card{c}/midi{d}")));
            let name = match (card.is_empty(), port.is_empty()) {
                (false, false) => format!("{card}: {port}"),
                (false, true) => card,
                (true, false) => port,
                _ => String::new(),
            };
            v.push(PortInfo { id: format!("hw:{c},{d}"), name, path: e.path() });
        }
        v.sort_by(|a, b| a.id.cmp(&b.id));
        v
    }

    fn open(&mut self, p: &PortInfo) -> Result<Box<dyn MidiSink>, String> {
        use std::os::unix::ffi::OsStrExt;
        let c = std::ffi::CString::new(p.path.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
        // SAFETY: plain open(2) of a path we own a CString for
        let fd = unsafe { libc::open(c.as_ptr(), libc::O_WRONLY | libc::O_NONBLOCK | libc::O_CLOEXEC) };
        if fd < 0 {
            return Err(format!("{}: {}", p.path.display(), io::Error::last_os_error()));
        }
        // SAFETY: fd is a fresh descriptor we exclusively own
        Ok(Box::new(RawMidi { fd: unsafe { OwnedFd::from_raw_fd(fd) }, name: p.label() }))
    }
}

pub struct RawMidi {
    fd: OwnedFd,
    name: String,
}

impl MidiSink for RawMidi {
    fn write(&mut self, b: &[u8]) -> Result<usize, SinkError> {
        // SAFETY: write(2) from a valid slice to our fd
        let n = unsafe { libc::write(self.fd.as_raw_fd(), b.as_ptr().cast(), b.len()) };
        if n >= 0 {
            return Ok(n as usize);
        }
        let e = io::Error::last_os_error();
        match e.raw_os_error() {
            Some(libc::EAGAIN) | Some(libc::EINTR) => Err(SinkError::WouldBlock),
            _ => Err(SinkError::Gone(e.to_string())),
        }
    }
    fn name(&self) -> String {
        self.name.clone()
    }
}

// ---------------------------------------------------------------- test sink

/// Records bytes; can be made unavailable, busy or to fail, for tests.
#[derive(Clone, Default)]
pub struct FakeSink {
    pub bytes: Arc<Mutex<Vec<u8>>>,
    /// next N writes return WouldBlock
    pub busy: Arc<AtomicU32>,
    /// writes fail with Gone (port unplugged)
    pub gone: Arc<AtomicBool>,
}

impl MidiSink for FakeSink {
    fn write(&mut self, b: &[u8]) -> Result<usize, SinkError> {
        if self.gone.load(Ordering::Relaxed) {
            return Err(SinkError::Gone("unplugged".into()));
        }
        if self.busy.load(Ordering::Relaxed) > 0 {
            self.busy.fetch_sub(1, Ordering::Relaxed);
            return Err(SinkError::WouldBlock);
        }
        self.bytes.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn name(&self) -> String {
        "fake".into()
    }
}

/// Opener for a [`FakeSink`] that can be plugged in and out.
#[derive(Clone, Default)]
pub struct FakeOpener {
    pub sink: FakeSink,
    pub present: Arc<AtomicBool>,
    pub opens: Arc<AtomicU32>,
}

impl Opener for FakeOpener {
    fn list(&mut self) -> Vec<PortInfo> {
        if self.present.load(Ordering::Relaxed) {
            vec![PortInfo { id: "fake:0".into(), name: "fake".into(), path: PathBuf::new() }]
        } else {
            vec![]
        }
    }
    fn open(&mut self, _p: &PortInfo) -> Result<Box<dyn MidiSink>, String> {
        if !self.present.load(Ordering::Relaxed) {
            return Err("not present".into());
        }
        self.opens.fetch_add(1, Ordering::Relaxed);
        self.sink.gone.store(false, Ordering::Relaxed);
        Ok(Box::new(self.sink.clone()))
    }
}
