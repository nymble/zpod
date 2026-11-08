//! Worker thread: takes messages off the queue and writes them at their due
//! time. Low priority (nice +5), sleeps when idle, never blocks the render
//! thread. Port handling:
//! * no port / unplugged: state `Unavailable`; the queue is still drained
//!   (messages counted as `unsent`), and the port list is rescanned every
//!   second. The app reads `connected()` to choose its fallback;
//! * busy port (EAGAIN): retry for up to 20 ms, then drop the message, count a
//!   write error and schedule a cleanup;
//! * cleanup (after drops, a lost queue message, a reconnect, and at
//!   shutdown): note-off for every note this worker actually sent and has
//!   not ended, then CC 123 on each channel it used.

use crate::config::PortName;
use crate::encode::{Msg, CC_ALL_NOTES_OFF, NOTE_OFF, NOTE_ON};
use crate::queue::{Item, Ring};
use crate::sink::{MidiSink, Opener};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub const QUEUE_CAP: usize = 1024;
const RETRY: Duration = Duration::from_millis(20);
const RESCAN: Duration = Duration::from_millis(1000);
/// messages older than this when they come off the queue are still sent
/// (note-offs matter) but counted as late
const LATE_NS: u64 = 50_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// route is Internal: no port opened
    Off,
    /// MIDI wanted but no matching port
    Unavailable,
    Connected,
}

#[derive(Default)]
pub struct Status {
    state: AtomicU8,
    pub sent: AtomicU64,
    pub unsent: AtomicU64,
    pub write_errors: AtomicU64,
    pub late: AtomicU64,
    pub reconnects: AtomicU64,
    pub cleanups: AtomicU64,
    /// connected port label, or the last error (UI thread reads; bounded)
    pub detail: Mutex<String>,
    /// ports seen at the last scan (bounded to 8 labels)
    pub ports: Mutex<Vec<String>>,
}

impl Status {
    pub fn state(&self) -> State {
        match self.state.load(Ordering::Relaxed) {
            1 => State::Unavailable,
            2 => State::Connected,
            _ => State::Off,
        }
    }
    fn set(&self, s: State, detail: &str) {
        self.state.store(s as u8, Ordering::Relaxed);
        if let Ok(mut d) = self.detail.lock() {
            d.clear();
            d.extend(detail.chars().take(40));
        }
    }
    pub fn detail(&self) -> String {
        self.detail.lock().map(|d| d.clone()).unwrap_or_default()
    }
}

/// What the app wants (UI thread writes, worker reads).
struct Want {
    enabled: AtomicBool,
    port: Mutex<PortName>,
    rescan: AtomicBool,
}

pub struct MidiOut {
    pub ring: Arc<Ring>,
    pub status: Arc<Status>,
    want: Arc<Want>,
    stop: Arc<AtomicBool>,
    epoch: Instant,
    join: Option<JoinHandle<()>>,
}

impl MidiOut {
    pub fn start(opener: Box<dyn Opener>) -> MidiOut {
        let ring = Arc::new(Ring::new(QUEUE_CAP));
        let status = Arc::new(Status::default());
        let want = Arc::new(Want { enabled: AtomicBool::new(false), port: Mutex::new(PortName::AUTO), rescan: AtomicBool::new(true) });
        let stop = Arc::new(AtomicBool::new(false));
        let epoch = Instant::now();
        let w = Worker { ring: ring.clone(), status: status.clone(), want: want.clone(), stop: stop.clone(), epoch, opener, sink: None, on: [0; 16], used: 0, need_cleanup: false };
        let join = thread::Builder::new().name("zpod-midi".into()).spawn(move || w.run()).ok();
        MidiOut { ring, status, want, stop, epoch, join }
    }
    /// Monotonic ns since this output's epoch (for `Router` due times).
    pub fn now_ns(&self) -> u64 {
        self.epoch.elapsed().as_nanos() as u64
    }
    pub fn epoch(&self) -> Instant {
        self.epoch
    }
    /// Open (or close) the port: call with the config's route and port.
    pub fn set_wanted(&self, enabled: bool, port: PortName) {
        if let Ok(mut p) = self.want.port.lock() {
            if *p != port {
                *p = port;
                self.want.rescan.store(true, Ordering::Relaxed);
            }
        }
        if self.want.enabled.swap(enabled, Ordering::Relaxed) != enabled {
            self.want.rescan.store(true, Ordering::Relaxed);
        }
    }
    pub fn rescan(&self) {
        self.want.rescan.store(true, Ordering::Relaxed);
    }
    pub fn connected(&self) -> bool {
        self.status.state() == State::Connected
    }
    /// Drain, clean up and close (also on drop).
    pub fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

impl Drop for MidiOut {
    fn drop(&mut self) {
        self.shutdown();
    }
}

struct Worker {
    ring: Arc<Ring>,
    status: Arc<Status>,
    want: Arc<Want>,
    stop: Arc<AtomicBool>,
    epoch: Instant,
    opener: Box<dyn Opener>,
    sink: Option<Box<dyn MidiSink>>,
    /// notes actually sent and not yet ended: bit per note, per channel
    on: [u128; 16],
    used: u16,
    need_cleanup: bool,
}

impl Worker {
    fn now(&self) -> u64 {
        self.epoch.elapsed().as_nanos() as u64
    }

    fn run(mut self) {
        lower_priority();
        let mut last_scan: Option<Instant> = None;
        let mut pending: Option<Item> = None;
        loop {
            let stopping = self.stop.load(Ordering::Relaxed);
            let enabled = self.want.enabled.load(Ordering::Relaxed);
            // port management
            if !enabled && self.sink.is_some() {
                self.cleanup();
                self.sink = None;
            }
            if !enabled {
                self.status.set(State::Off, "");
            } else if self.sink.is_none() && !stopping {
                let due = self.want.rescan.swap(false, Ordering::Relaxed) || last_scan.is_none_or(|t| t.elapsed() >= RESCAN);
                if due {
                    last_scan = Some(Instant::now());
                    self.connect();
                }
            } else if self.want.rescan.swap(false, Ordering::Relaxed) {
                // port choice changed: reconnect
                self.cleanup();
                self.sink = None;
                continue;
            }
            // messages
            if pending.is_none() {
                pending = self.ring.pop();
            }
            match pending {
                Some(it) => {
                    let now = self.now();
                    if it.due_ns > now && !stopping {
                        let wait = (it.due_ns - now).min(5_000_000);
                        thread::sleep(Duration::from_nanos(wait));
                        continue;
                    }
                    if now > it.due_ns + LATE_NS {
                        self.status.late.fetch_add(1, Ordering::Relaxed);
                    }
                    pending = None;
                    self.send(it.msg, true);
                }
                None => {
                    if self.ring.lost.swap(false, Ordering::AcqRel) {
                        self.need_cleanup = true;
                    }
                    if self.need_cleanup {
                        self.cleanup();
                    }
                    if stopping {
                        self.cleanup();
                        self.sink = None;
                        self.status.set(State::Off, "");
                        return;
                    }
                    // idle: new messages are due ~latency in the future, so a
                    // short nap costs no timing; long nap when routing is off
                    thread::sleep(Duration::from_millis(if enabled { 3 } else { 40 }));
                }
            }
        }
    }

    fn connect(&mut self) {
        let want = self.want.port.lock().map(|p| *p).unwrap_or(PortName::AUTO);
        let ports = self.opener.list();
        if let Ok(mut l) = self.status.ports.lock() {
            *l = ports.iter().take(8).map(|p| p.label()).collect();
        }
        let Some(p) = ports.iter().find(|p| p.matches(want.as_str())) else {
            self.status.set(State::Unavailable, if ports.is_empty() { "no MIDI port" } else { "chosen port not found" });
            return;
        };
        match self.opener.open(p) {
            Ok(s) => {
                self.status.set(State::Connected, &s.name());
                self.status.reconnects.fetch_add(1, Ordering::Relaxed);
                self.sink = Some(s);
                // whatever the device had from before is unknown: reset it
                self.on = [0; 16];
                self.need_cleanup = true;
            }
            Err(e) => self.status.set(State::Unavailable, &e),
        }
    }

    /// Write one message (retrying a busy port briefly). `track`: update the
    /// sounding-note table from what was actually written.
    fn send(&mut self, m: Msg, track: bool) -> bool {
        let Some(sink) = self.sink.as_mut() else {
            self.status.unsent.fetch_add(1, Ordering::Relaxed);
            return false;
        };
        let b = m.bytes();
        let mut done = 0;
        let t0 = Instant::now();
        while done < b.len() {
            match sink.write(&b[done..]) {
                Ok(n) => done += n,
                Err(crate::sink::SinkError::WouldBlock) => {
                    if t0.elapsed() > RETRY {
                        break;
                    }
                    thread::sleep(Duration::from_millis(1));
                }
                Err(crate::sink::SinkError::Gone(e)) => {
                    self.sink = None;
                    self.status.set(State::Unavailable, &format!("lost: {e}"));
                    self.status.write_errors.fetch_add(1, Ordering::Relaxed);
                    self.on = [0; 16];
                    return false;
                }
            }
        }
        if done < b.len() {
            // dropped after 20 ms busy (a partial message is resynchronised by
            // the next status byte); end whatever may now hang
            self.status.write_errors.fetch_add(1, Ordering::Relaxed);
            self.need_cleanup = true;
            return false;
        }
        self.status.sent.fetch_add(1, Ordering::Relaxed);
        if track {
            if let Some(ch) = m.channel() {
                let c = ch as usize - 1;
                self.used |= 1 << c;
                let kind = m.b[0] & 0xf0;
                let bit = 1u128 << (m.b[1] & 0x7f);
                if kind == NOTE_ON && m.b[2] > 0 {
                    self.on[c] |= bit;
                } else if kind == NOTE_OFF || kind == NOTE_ON {
                    self.on[c] &= !bit;
                }
            }
        }
        true
    }

    fn cleanup(&mut self) {
        self.need_cleanup = false;
        if self.sink.is_none() {
            return;
        }
        self.status.cleanups.fetch_add(1, Ordering::Relaxed);
        for c in 0..16u8 {
            let mut bits = self.on[c as usize];
            while bits != 0 {
                let n = bits.trailing_zeros() as u8;
                bits &= bits - 1;
                self.send(Msg::note_off(c + 1, n), true);
            }
            if self.used & (1 << c) != 0 {
                self.send(Msg::cc(c + 1, CC_ALL_NOTES_OFF, 0), false);
            }
        }
        self.on = [0; 16];
    }
}

fn lower_priority() {
    // SAFETY: setpriority on this thread's own tid (Linux: per-thread nice)
    unsafe {
        let tid = libc::syscall(libc::SYS_gettid) as libc::id_t;
        libc::setpriority(libc::PRIO_PROCESS, tid, 5);
    }
}
