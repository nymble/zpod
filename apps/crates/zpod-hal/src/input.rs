//! Button input from the `zpod-buttons` uinput keyboard (evdev).
//!
//! The device is found by name through sysfs (no ioctl needed), opened
//! non-blocking and never grabbed, so the launcher can watch for a long Home
//! press while an app runs. Reconnects if the daemon restarts.

use std::fs::File;
use std::io::Read;
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use zpod_core::{Event, Key, KeyState};

pub const DEVICE_NAME: &str = "zpod-buttons";
const EV_KEY: u16 = 0x01;
/// sizeof(struct input_event): timeval (2 x long) + type + code + value.
/// 16 bytes on 32-bit ARM (also with 64-bit time_t userspace), 24 on x86_64.
const EVENT_SIZE: usize = 2 * std::mem::size_of::<libc::c_ulong>() + 8;

pub fn find_device(name: &str) -> Option<PathBuf> {
    if let Ok(p) = std::env::var("ZPOD_INPUT") {
        return Some(p.into());
    }
    let mut entries: Vec<_> = std::fs::read_dir("/sys/class/input").ok()?.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let fname = e.file_name().to_string_lossy().into_owned();
        if !fname.starts_with("event") {
            continue;
        }
        let n = std::fs::read_to_string(e.path().join("device/name")).unwrap_or_default();
        if n.trim() == name {
            return Some(PathBuf::from("/dev/input").join(fname));
        }
    }
    None
}

/// Decode one raw input_event; returns (type, code, value).
pub fn decode(raw: &[u8]) -> (u16, u16, i32) {
    let o = EVENT_SIZE - 8;
    let t = u16::from_ne_bytes([raw[o], raw[o + 1]]);
    let c = u16::from_ne_bytes([raw[o + 2], raw[o + 3]]);
    let v = i32::from_ne_bytes([raw[o + 4], raw[o + 5], raw[o + 6], raw[o + 7]]);
    (t, c, v)
}

pub fn to_event(t: u16, code: u16, value: i32) -> Option<Event> {
    if t != EV_KEY {
        return None;
    }
    let key = Key::from_linux_code(code)?;
    let state = match value {
        0 => KeyState::Release,
        1 => KeyState::Press,
        2 => KeyState::Repeat,
        _ => return None,
    };
    Some(Event { key, state })
}

pub struct EvdevInput {
    dev: Option<File>,
    next_try: Instant,
    pending: std::collections::VecDeque<Event>,
}

impl Default for EvdevInput {
    fn default() -> Self {
        Self::new()
    }
}

impl EvdevInput {
    pub fn new() -> Self {
        let mut s = EvdevInput { dev: None, next_try: Instant::now(), pending: Default::default() };
        s.ensure();
        s
    }

    pub fn connected(&self) -> bool {
        self.dev.is_some()
    }

    fn ensure(&mut self) {
        if self.dev.is_some() || Instant::now() < self.next_try {
            return;
        }
        self.next_try = Instant::now() + Duration::from_secs(2);
        if let Some(p) = find_device(DEVICE_NAME) {
            use std::os::unix::fs::OpenOptionsExt;
            if let Ok(f) = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK).open(&p) {
                eprintln!("input: {} ({DEVICE_NAME})", p.display());
                self.dev = Some(f);
            }
        }
    }

    /// Drop queued and buffered events (e.g. after an app exits).
    pub fn drain(&mut self) {
        self.pending.clear();
        while self.next_event(Some(Duration::ZERO)).is_some() {}
        self.pending.clear();
    }

    /// Wait up to `timeout` (None = forever) for the next key event.
    pub fn next_event(&mut self, timeout: Option<Duration>) -> Option<Event> {
        if let Some(e) = self.pending.pop_front() {
            return Some(e);
        }
        let deadline = timeout.map(|t| Instant::now() + t);
        loop {
            self.ensure();
            let remaining = deadline.map(|d| d.saturating_duration_since(Instant::now()));
            let Some(dev) = self.dev.as_mut() else {
                // No device: sleep in 2 s slices until it appears or timeout.
                let s = remaining.unwrap_or(Duration::from_secs(2)).min(Duration::from_secs(2));
                if s.is_zero() {
                    return None;
                }
                std::thread::sleep(s);
                if deadline.is_some_and(|d| Instant::now() >= d) {
                    return None;
                }
                continue;
            };
            let ms: i32 = match remaining {
                None => 2000,
                Some(r) => r.as_millis().min(2000) as i32,
            };
            let mut pfd = libc::pollfd { fd: dev.as_raw_fd(), events: libc::POLLIN, revents: 0 };
            let rc = unsafe { libc::poll(&mut pfd, 1, ms) };
            if rc > 0 {
                if pfd.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
                    eprintln!("input: device lost; reconnecting");
                    self.dev = None;
                    continue;
                }
                let mut buf = [0u8; EVENT_SIZE * 16];
                match dev.read(&mut buf) {
                    Ok(n) => {
                        for raw in buf[..n].chunks_exact(EVENT_SIZE) {
                            let (t, c, v) = decode(raw);
                            if let Some(e) = to_event(t, c, v) {
                                self.pending.push_back(e);
                            }
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(_) => {
                        self.dev = None;
                        continue;
                    }
                }
                if let Some(e) = self.pending.pop_front() {
                    return Some(e);
                }
            }
            if deadline.is_some_and(|d| Instant::now() >= d) {
                return None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_native_input_event() {
        let mut raw = vec![0u8; EVENT_SIZE];
        let o = EVENT_SIZE - 8;
        raw[o..o + 2].copy_from_slice(&1u16.to_ne_bytes());
        raw[o + 2..o + 4].copy_from_slice(&37u16.to_ne_bytes()); // KEY_K
        raw[o + 4..o + 8].copy_from_slice(&1i32.to_ne_bytes());
        let (t, c, v) = decode(&raw);
        assert_eq!(to_event(t, c, v), Some(Event { key: Key::Circle, state: KeyState::Press }));
        assert_eq!(to_event(0, 0, 0), None); // EV_SYN ignored
        assert_eq!(to_event(1, 28, 2).unwrap().key, Key::Ok);
    }

    #[test]
    fn event_size_matches_arch() {
        #[cfg(target_pointer_width = "32")]
        assert_eq!(EVENT_SIZE, 16);
        #[cfg(target_pointer_width = "64")]
        assert_eq!(EVENT_SIZE, 24);
    }
}
