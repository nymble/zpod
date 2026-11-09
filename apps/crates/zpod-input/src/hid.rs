//! USB / Bluetooth keyboards and mice: a reader thread with hotplug.
//!
//! `HidInput::spawn()` starts one thread that rescans `/sys/class/input`
//! every second, opens new keyboards/mice read-only and non-blocking (never
//! grabbed), switches their timestamps to `CLOCK_MONOTONIC`, normalizes
//! their events and sends [`HidEvent`]s over a channel. Unplugging a device
//! ends its open contacts and sends `Removed`. Apps poll with
//! [`HidInput::poll`] from their own loop (no blocking, buttons keep working).
//!
//! `ZPOD_HID_SIM=<script>` replaces the devices with a script (host
//! simulator, screenshots, tests); one command per line:
//!
//! ```text
//! wait <ms>                 let time pass
//! add keyboard|mouse <name> plug a device in (names may have spaces)
//! remove keyboard|mouse     unplug it
//! key <name> [down|up]      press + release (or only one edge): enter, esc, a, f1 ...
//! type <text>               tap the keys that type <text> (Shift as needed)
//! move <dx> <dy>            relative pointer motion (one report)
//! click left|right|middle   press + release
//! wheel <n>                 wheel detents (+ = up)
//! # comment
//! ```
//! `ZPOD_HID_SIM=none` = no devices at all (also no sysfs scanning).

use std::collections::HashMap;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::devices::{self, Candidate};
use crate::event::*;
use crate::keymap;
use crate::normalize::{self, Normalizer};

/// sizeof(struct input_event): timeval (2 x long) + type + code + value.
const EVENT_SIZE: usize = 2 * std::mem::size_of::<libc::c_ulong>() + 8;
/// EVIOCSCLOCKID = _IOW('E', 0xa0, int)
const EVIOCSCLOCKID: libc::c_ulong = 0x4004_45a0;

pub struct HidInput {
    rx: Receiver<HidEvent>,
    devices: Arc<Mutex<Vec<DeviceInfo>>>,
    /// set on drop: the reader thread exits within ~1 s
    stop: Arc<AtomicBool>,
}

impl Drop for HidInput {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl HidInput {
    /// Real devices, or the simulator when `ZPOD_HID_SIM` is set.
    pub fn spawn() -> HidInput {
        let devices = Arc::new(Mutex::new(Vec::new()));
        let (tx, rx) = channel();
        let d2 = devices.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let st2 = stop.clone();
        match std::env::var("ZPOD_HID_SIM") {
            Ok(s) if s == "none" => {}
            Ok(script) => {
                let text = std::fs::read_to_string(&script).unwrap_or_default();
                std::thread::Builder::new().name("hid-sim".into()).spawn(move || run_sim(&text, tx, d2, true)).ok();
            }
            Err(_) => {
                std::thread::Builder::new()
                    .name("hid".into())
                    .spawn(move || run_reader(Path::new("/sys/class/input"), Path::new("/dev/input"), tx, d2, st2))
                    .ok();
            }
        }
        HidInput { rx, devices, stop }
    }

    /// From a script string (tests). `realtime` false = waits are skipped
    /// (timestamps still advance).
    pub fn from_script(script: &str, realtime: bool) -> HidInput {
        let devices = Arc::new(Mutex::new(Vec::new()));
        let (tx, rx) = channel();
        let d2 = devices.clone();
        let s = script.to_string();
        std::thread::spawn(move || run_sim(&s, tx, d2, realtime));
        HidInput { rx, devices, stop: Arc::new(AtomicBool::new(false)) }
    }

    /// A reader that never produces anything (buttons-only).
    pub fn none() -> HidInput {
        let (_tx, rx) = channel();
        HidInput { rx, devices: Arc::new(Mutex::new(Vec::new())), stop: Arc::new(AtomicBool::new(false)) }
    }

    /// Next pending event, if any (never blocks).
    pub fn poll(&self) -> Option<HidEvent> {
        self.rx.try_recv().ok()
    }

    /// Wait up to `timeout` for the next event.
    pub fn wait(&self, timeout: Duration) -> Option<HidEvent> {
        self.rx.recv_timeout(timeout).ok()
    }

    pub fn devices(&self) -> Vec<DeviceInfo> {
        self.devices.lock().map(|d| d.clone()).unwrap_or_default()
    }

    pub fn has(&self, kind: SourceKind) -> bool {
        self.devices().iter().any(|d| d.kind == kind)
    }
}

struct Open {
    info: DeviceInfo,
    file: std::fs::File,
    norm: Normalizer,
    mono_ts: bool,
}

fn decode(raw: &[u8]) -> (u64, u16, u16, i32) {
    let l = std::mem::size_of::<libc::c_ulong>();
    let rd = |o: usize| -> u64 {
        let mut b = [0u8; 8];
        b[..l].copy_from_slice(&raw[o..o + l]);
        u64::from_ne_bytes(b)
    };
    let t = rd(0) * 1_000_000 + rd(l);
    let o = EVENT_SIZE - 8;
    let ty = u16::from_ne_bytes([raw[o], raw[o + 1]]);
    let c = u16::from_ne_bytes([raw[o + 2], raw[o + 3]]);
    let v = i32::from_ne_bytes([raw[o + 4], raw[o + 5], raw[o + 6], raw[o + 7]]);
    (t, ty, c, v)
}

fn open(c: &Candidate, id: SourceId) -> Option<Open> {
    use std::os::unix::fs::OpenOptionsExt;
    let file = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC).open(&c.dev).ok()?;
    let clk: libc::c_int = libc::CLOCK_MONOTONIC;
    let mono_ts = unsafe { libc::ioctl(file.as_raw_fd(), EVIOCSCLOCKID as _, &clk) } == 0;
    let info = DeviceInfo { id, kind: c.kind, name: c.name.clone(), path: c.dev.display().to_string(), bus: c.bus, vendor: c.vendor, product: c.product };
    Some(Open { info, file, norm: Normalizer::new(id), mono_ts })
}

fn publish(devs: &Arc<Mutex<Vec<DeviceInfo>>>, open: &HashMap<PathBuf, Open>) {
    let mut v: Vec<DeviceInfo> = open.values().map(|o| o.info.clone()).collect();
    v.sort_by_key(|d| d.id);
    if let Ok(mut d) = devs.lock() {
        *d = v;
    }
}

fn run_reader(sys: &Path, dev: &Path, tx: Sender<HidEvent>, devs: Arc<Mutex<Vec<DeviceInfo>>>, stop: Arc<AtomicBool>) {
    let mut open_devs: HashMap<PathBuf, Open> = HashMap::new();
    let mut next_id: SourceId = 1;
    let mut next_scan = Instant::now();
    let mut out = Vec::new();
    loop {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        if Instant::now() >= next_scan {
            next_scan = Instant::now() + Duration::from_secs(1);
            for c in devices::scan(sys, dev) {
                if open_devs.contains_key(&c.dev) {
                    continue;
                }
                if let Some(o) = open(&c, next_id) {
                    next_id += 1;
                    eprintln!("zpod-input: {} {} ({}, {:04x}:{:04x}) at {}", o.info.bus_label(), o.info.kind.label(), o.info.name, o.info.vendor, o.info.product, o.info.path);
                    if tx.send(HidEvent::Added(o.info.clone())).is_err() {
                        return;
                    }
                    open_devs.insert(c.dev.clone(), o);
                    publish(&devs, &open_devs);
                }
            }
        }
        let keys: Vec<PathBuf> = open_devs.keys().cloned().collect();
        let mut fds: Vec<libc::pollfd> = keys.iter().map(|k| libc::pollfd { fd: open_devs[k].file.as_raw_fd(), events: libc::POLLIN, revents: 0 }).collect();
        if fds.is_empty() {
            std::thread::sleep(next_scan.saturating_duration_since(Instant::now()));
            continue;
        }
        let rc = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, 100) };
        let now = crate::now_us();
        let mut gone = vec![];
        for (i, k) in keys.iter().enumerate() {
            let o = open_devs.get_mut(k).unwrap();
            if rc > 0 && fds[i].revents != 0 {
                let mut buf = [0u8; EVENT_SIZE * 32];
                let n = unsafe { libc::read(o.file.as_raw_fd(), buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
                if n > 0 {
                    for raw in buf[..n as usize].chunks_exact(EVENT_SIZE) {
                        let (t, ty, c, v) = decode(raw);
                        o.norm.feed(if o.mono_ts { t } else { now }, ty, c, v, &mut out);
                    }
                } else if fds[i].revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 || n == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::ENODEV) {
                    gone.push(k.clone());
                    continue;
                }
            }
            o.norm.tick(now, &mut out);
        }
        for k in gone {
            if let Some(mut o) = open_devs.remove(&k) {
                o.norm.unplug(now, &mut out);
                for e in out.drain(..) {
                    let _ = tx.send(HidEvent::Input(e));
                }
                eprintln!("zpod-input: {} removed ({})", o.info.kind.label(), o.info.name);
                let _ = tx.send(HidEvent::Removed(o.info));
                publish(&devs, &open_devs);
            }
        }
        for e in out.drain(..) {
            if tx.send(HidEvent::Input(e)).is_err() {
                return;
            }
        }
    }
}

/// Scripted devices (see module docs).
pub fn run_sim(script: &str, tx: Sender<HidEvent>, devs: Arc<Mutex<Vec<DeviceInfo>>>, realtime: bool) {
    let t0 = crate::now_us();
    let mut virt: u64 = 0; // µs since start when not realtime
    let now = |virt: u64| if realtime { crate::now_us() } else { t0 + virt };
    let mut by_kind: HashMap<&'static str, (DeviceInfo, Normalizer)> = HashMap::new();
    let mut next_id: SourceId = 1;
    let mut out = Vec::new();
    let publish = |m: &HashMap<&'static str, (DeviceInfo, Normalizer)>| {
        let mut v: Vec<DeviceInfo> = m.values().map(|x| x.0.clone()).collect();
        v.sort_by_key(|d| d.id);
        if let Ok(mut d) = devs.lock() {
            *d = v;
        }
    };
    for line in script.lines() {
        let line = line.trim();
        let mut it = line.splitn(2, char::is_whitespace);
        let cmd = it.next().unwrap_or("");
        let rest = it.next().unwrap_or("").trim();
        let kind_key = |s: &str| -> Option<&'static str> {
            match s.split_whitespace().next() {
                Some("keyboard") => Some("keyboard"),
                Some("mouse") => Some("mouse"),
                _ => None,
            }
        };
        let step = 8_000u64; // 8 ms between scripted reports
        match cmd {
            "" => continue,
            c if c.starts_with('#') => continue,
            "wait" => {
                let ms: u64 = rest.parse().unwrap_or(0);
                // keep pointer idle tracking honest while waiting
                let mut left = ms;
                while left > 0 {
                    let s = left.min(50);
                    if realtime {
                        std::thread::sleep(Duration::from_millis(s));
                    }
                    virt += s * 1000;
                    left -= s;
                    for (_, n) in by_kind.values_mut() {
                        n.tick(now(virt), &mut out);
                    }
                    for e in out.drain(..) {
                        let _ = tx.send(HidEvent::Input(e));
                    }
                }
                continue;
            }
            "add" => {
                if let Some(k) = kind_key(rest) {
                    let name = rest[k.len()..].trim();
                    let kind = if k == "mouse" { SourceKind::Mouse } else { SourceKind::Keyboard };
                    let info = DeviceInfo {
                        id: next_id,
                        kind,
                        name: if name.is_empty() { format!("Sim {k}") } else { name.to_string() },
                        path: "sim".into(),
                        bus: 0x03,
                        vendor: 0,
                        product: 0,
                    };
                    by_kind.insert(k, (info.clone(), Normalizer::new(next_id)));
                    next_id += 1;
                    publish(&by_kind);
                    let _ = tx.send(HidEvent::Added(info));
                }
            }
            "remove" => {
                if let Some(k) = kind_key(rest) {
                    if let Some((info, mut n)) = by_kind.remove(k) {
                        n.unplug(now(virt), &mut out);
                        for e in out.drain(..) {
                            let _ = tx.send(HidEvent::Input(e));
                        }
                        publish(&by_kind);
                        let _ = tx.send(HidEvent::Removed(info));
                    }
                }
            }
            "key" | "type" => {
                let Some((_, n)) = by_kind.get_mut("keyboard") else { continue };
                let mut taps: Vec<(u16, bool, Option<bool>)> = vec![]; // code, shift, edge
                if cmd == "key" {
                    let mut p = rest.split_whitespace();
                    let name = p.next().unwrap_or("");
                    let edge = match p.next() {
                        Some("down") => Some(true),
                        Some("up") => Some(false),
                        _ => None,
                    };
                    if let Some(c) = keymap::code_for_name(name) {
                        taps.push((c, false, edge));
                    }
                } else {
                    for ch in rest.chars() {
                        if let Some((c, sh)) = keymap::code_for_char(ch) {
                            taps.push((c, sh, None));
                        }
                    }
                }
                for (code, shift, edge) in taps {
                    let mut emit = |ty, c, v, virt: &mut u64| {
                        *virt += step;
                        n.feed(now(*virt), ty, c, v, &mut out);
                        n.feed(now(*virt), normalize::EV_SYN, normalize::SYN_REPORT, 0, &mut out);
                    };
                    if shift {
                        emit(normalize::EV_KEY, keymap::KEY_LEFTSHIFT, 1, &mut virt);
                    }
                    if edge != Some(false) {
                        emit(normalize::EV_KEY, code, 1, &mut virt);
                    }
                    if edge != Some(true) {
                        emit(normalize::EV_KEY, code, 0, &mut virt);
                    }
                    if shift {
                        emit(normalize::EV_KEY, keymap::KEY_LEFTSHIFT, 0, &mut virt);
                    }
                }
            }
            "move" | "click" | "wheel" => {
                let Some((_, n)) = by_kind.get_mut("mouse") else { continue };
                virt += step;
                let t = now(virt);
                let mut p = rest.split_whitespace();
                match cmd {
                    "move" => {
                        let dx: i32 = p.next().and_then(|v| v.parse().ok()).unwrap_or(0);
                        let dy: i32 = p.next().and_then(|v| v.parse().ok()).unwrap_or(0);
                        n.feed(t, normalize::EV_REL, normalize::REL_X, dx, &mut out);
                        n.feed(t, normalize::EV_REL, normalize::REL_Y, dy, &mut out);
                    }
                    "wheel" => {
                        let d: i32 = p.next().and_then(|v| v.parse().ok()).unwrap_or(1);
                        n.feed(t, normalize::EV_REL, normalize::REL_WHEEL, d, &mut out);
                    }
                    _ => {
                        let b = match p.next() {
                            Some("right") => normalize::BTN_RIGHT,
                            Some("middle") => normalize::BTN_MIDDLE,
                            _ => normalize::BTN_LEFT,
                        };
                        n.feed(t, normalize::EV_KEY, b, 1, &mut out);
                        n.feed(t, normalize::EV_SYN, normalize::SYN_REPORT, 0, &mut out);
                        virt += step;
                        n.feed(now(virt), normalize::EV_KEY, b, 0, &mut out);
                    }
                }
                n.feed(now(virt), normalize::EV_SYN, normalize::SYN_REPORT, 0, &mut out);
            }
            _ => eprintln!("zpod-input sim: bad line"),
        }
        for e in out.drain(..) {
            if tx.send(HidEvent::Input(e)).is_err() {
                return;
            }
        }
        if realtime {
            std::thread::sleep(Duration::from_millis(8));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect(script: &str) -> (Vec<HidEvent>, HidInput) {
        let h = HidInput::from_script(script, false);
        let mut v = vec![];
        while let Some(e) = h.wait(Duration::from_millis(300)) {
            v.push(e);
        }
        (v, h)
    }

    #[test]
    fn sim_typing_produces_text_only_through_events() {
        let (v, h) = collect("add keyboard Sim KB\ntype Ab1!\nkey enter\n");
        assert!(matches!(&v[0], HidEvent::Added(d) if d.kind == SourceKind::Keyboard && d.name == "Sim KB"));
        let text: String = v.iter().filter_map(|e| match e {
            HidEvent::Input(i) if i.phase == Phase::Begin => i.text,
            _ => None,
        }).collect();
        assert_eq!(text, "Ab1!");
        let enter = v.iter().any(|e| matches!(e, HidEvent::Input(i) if i.control == Control::Key(keymap::KeyCode::Enter) && i.phase == Phase::Begin));
        assert!(enter);
        assert!(h.has(SourceKind::Keyboard));
        let ts: Vec<u64> = v.iter().filter_map(|e| if let HidEvent::Input(i) = e { Some(i.t_us) } else { None }).collect();
        assert!(ts.windows(2).all(|w| w[0] <= w[1]), "monotonic timestamps");
    }

    #[test]
    fn sim_mouse_hotplug_and_idle_end() {
        let (v, h) = collect("add mouse Sim Mouse\nmove 10 -4\nmove 2 0\nwait 400\nclick left\nwheel 2\nremove mouse\n");
        let inputs: Vec<&InputEvent> = v.iter().filter_map(|e| if let HidEvent::Input(i) = e { Some(i) } else { None }).collect();
        let ptr: Vec<_> = inputs.iter().filter(|i| i.control == Control::Pointer).map(|i| i.phase).collect();
        assert_eq!(ptr, vec![Phase::Begin, Phase::Update, Phase::End]);
        assert!(inputs.iter().any(|i| i.control == Control::Button(PointerButton::Left) && i.phase == Phase::Begin));
        assert!(inputs.iter().any(|i| i.control == Control::Wheel && i.dy == 2));
        assert!(matches!(v.last(), Some(HidEvent::Removed(d)) if d.kind == SourceKind::Mouse));
        assert!(!h.has(SourceKind::Mouse));
    }

    #[test]
    fn decode_native_event() {
        let mut raw = vec![0u8; EVENT_SIZE];
        let l = std::mem::size_of::<libc::c_ulong>();
        raw[..l].copy_from_slice(&(3 as libc::c_ulong).to_ne_bytes());
        raw[l..2 * l].copy_from_slice(&(250 as libc::c_ulong).to_ne_bytes());
        let o = EVENT_SIZE - 8;
        raw[o..o + 2].copy_from_slice(&2u16.to_ne_bytes());
        raw[o + 2..o + 4].copy_from_slice(&1u16.to_ne_bytes());
        raw[o + 4..o + 8].copy_from_slice(&(-7i32).to_ne_bytes());
        assert_eq!(decode(&raw), (3_000_250, 2, 1, -7));
    }
}
