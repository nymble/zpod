// Copyright (C) 2026 Paul Lambert
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! USB role switching for Tools > USB and for apps (app socket).
//!
//! The work is done by the root tool `zpod-usb-role` (shell, see
//! buildroot/board/zpod/rootfs-overlay/usr/sbin/zpod-usb-role and
//! docs/USB-HOST.md): a live switch without a reboot (dwc2 unbind,
//! device-tree overlay through configfs, bind), RAM only, so a reboot always
//! restores the role on the boot partition. The launcher runs it as root on
//! a thread; apps only ask, through the launcher.

use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

use zpod_apps::ctl::UsbTarget;
use zpod_hal::sys;
use zpod_input::role::{self, RoleInfo};
use zpod_input::UsbRole;

/// The tool: `ZPOD_USB_ROLE_TOOL`, else the image's `/usr/sbin/zpod-usb-role`
/// (a newer copy from /data is bind-mounted over it by zpod-core-overlay
/// when core updates are active), else, on test3 (no such file in the
/// image), the copy `scripts/zpod-dev --core` pushed to /data (root-owned,
/// not writable by others).
pub fn tool() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("ZPOD_USB_ROLE_TOOL") {
        return Some(p.into());
    }
    let img = PathBuf::from("/usr/sbin/zpod-usb-role");
    if img.exists() {
        return Some(img);
    }
    let dev = PathBuf::from("/data/zpod/core/zpod-usb-role");
    trusted(&dev).then_some(dev)
}

/// Root-owned, executable, and neither it nor its directory writable by
/// anyone else (the launcher runs it as root).
fn trusted(p: &std::path::Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let ok = |m: &std::fs::Metadata| m.uid() == 0 && m.mode() & 0o022 == 0;
    match (std::fs::metadata(p), p.parent().map(std::fs::metadata)) {
        (Ok(m), Some(Ok(d))) => m.is_file() && ok(&m) && m.mode() & 0o100 != 0 && ok(&d),
        _ => false,
    }
}

/// Current state. Without the tool (older image): the role from the device
/// tree, no switching.
pub fn status() -> RoleInfo {
    status_all().0
}

/// USB MIDI next to USB Ethernet on the computer link (Tools > USB > USB
/// MIDI, `zpod-usb-role midi on|off`). `known`: the tool reports it (test7+).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Midi {
    pub known: bool,
    pub on: bool,
    pub live: bool,
}

impl Midi {
    pub fn parse(s: &str) -> Midi {
        let mut m = Midi::default();
        for l in s.lines() {
            match l.trim() {
                "midi=on" => (m.known, m.on) = (true, true),
                "midi=off" => (m.known, m.on) = (true, false),
                "midi_live=1" => m.live = true,
                _ => {}
            }
        }
        m
    }
}


/// USB HID keyboard next to USB Ethernet (Tools > USB > USB HID,
/// `zpod-usb-role hid on|off`). Used by HID Lab / DuckyScript.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Hid {
    pub known: bool,
    pub on: bool,
    pub live: bool,
}

impl Hid {
    pub fn parse(s: &str) -> Hid {
        let mut m = Hid::default();
        for l in s.lines() {
            match l.trim() {
                "hid=on" => (m.known, m.on) = (true, true),
                "hid=off" => (m.known, m.on) = (true, false),
                "hid_live=1" => m.live = true,
                _ => {}
            }
        }
        m
    }
}

/// [`status`] plus MIDI and HID switches, from one run of the tool.
pub fn status_all() -> (RoleInfo, Midi, Hid) {
    if sys::sim() {
        let on = std::env::var("ZPOD_SIM_USB_MIDI").as_deref() == Ok("on");
        let hon = std::env::var("ZPOD_SIM_USB_HID").as_deref() == Ok("on");
        return (role::sim_info(), Midi { known: true, on, live: on }, Hid { known: true, on: hon, live: hon });
    }
    if let Some(t) = tool() {
        if let Ok(o) = Command::new(&t).arg("status").output() {
            if o.status.success() {
                let out = String::from_utf8_lossy(&o.stdout);
                let i = RoleInfo::parse(&out);
                if i.role.is_some() {
                    return (i, Midi::parse(&out), Hid::parse(&out));
                }
            }
        }
    }
    let r = role::usb_role();
    (RoleInfo { role: Some(r), boot: Some(r), next: Some(r), persist: "none".into(), ..Default::default() }, Midi::default(), Hid::default())
}

pub fn status_midi() -> (RoleInfo, Midi) {
    let (i, m, _) = status_all();
    (i, m)
}

/// Turn USB MIDI on or off (blocking, ~1-2 s: restarts the computer link
/// when it is up). Ok(message) or Err(reason).
pub fn set_midi(on: bool) -> Result<String, String> {
    let w = if on { "on" } else { "off" };
    if sys::sim() {
        std::thread::sleep(Duration::from_millis(300));
        std::env::set_var("ZPOD_SIM_USB_MIDI", w);
        let host = std::env::var("ZPOD_USB_ROLE").as_deref() == Ok("host");
        return Ok(if host { format!("midi {w} (applies when USB is next the computer link)") } else { format!("midi {w} (computer link restarted)") });
    }
    let t = tool().ok_or("this image has no zpod-usb-role")?;
    let o = Command::new(&t).args(["midi", w]).output().map_err(|e| format!("{}: {e}", t.display()))?;
    let out = String::from_utf8_lossy(&o.stdout);
    let last = out.lines().filter(|l| l.starts_with("ok") || l.starts_with("err")).last().unwrap_or("").trim().to_string();
    eprintln!("launcher: usb midi {w}: {}", if last.is_empty() { "no answer" } else { &last });
    if let Some(r) = last.strip_prefix("ok") {
        Ok(r.trim().to_string())
    } else if let Some(r) = last.strip_prefix("err") {
        Err(r.trim().to_string())
    } else {
        Err(format!("zpod-usb-role exited {}", o.status.code().unwrap_or(-1)))
    }
}

/// A USB MIDI on/off running on a thread.

pub fn set_hid(on: bool) -> Result<String, String> {
    let w = if on { "on" } else { "off" };
    if sys::sim() {
        std::thread::sleep(Duration::from_millis(300));
        std::env::set_var("ZPOD_SIM_USB_HID", w);
        let host = std::env::var("ZPOD_USB_ROLE").as_deref() == Ok("host");
        return Ok(if host { format!("hid {w} (applies when USB is next the computer link)") } else { format!("hid {w} (computer link restarted)") });
    }
    let t = tool().ok_or("this image has no zpod-usb-role")?;
    let o = Command::new(&t).args(["hid", w]).output().map_err(|e| format!("{}: {e}", t.display()))?;
    let out = String::from_utf8_lossy(&o.stdout);
    let last = out.lines().filter(|l| l.starts_with("ok") || l.starts_with("err")).last().unwrap_or("").trim().to_string();
    eprintln!("launcher: usb hid {w}: {}", if last.is_empty() { "no answer" } else { &last });
    if let Some(r) = last.strip_prefix("ok") {
        Ok(r.trim().to_string())
    } else if let Some(r) = last.strip_prefix("err") {
        Err(r.trim().to_string())
    } else {
        Err(format!("zpod-usb-role exited {}", o.status.code().unwrap_or(-1)))
    }
}

pub struct MidiJob {
    pub on: bool,
    rx: Receiver<Result<String, String>>,
}

impl MidiJob {
    pub fn start(on: bool) -> MidiJob {
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(set_midi(on));
        });
        MidiJob { on, rx }
    }

    pub fn poll(&self) -> Option<Result<String, String>> {
        match self.rx.try_recv() {
            Ok(r) => Some(r),
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err("USB MIDI thread failed".into())),
        }
    }
}

pub struct HidJob {
    pub on: bool,
    rx: Receiver<Result<String, String>>,
    started: Instant,
}

impl HidJob {
    pub fn start(on: bool) -> HidJob {
        let (tx, rx) = channel();
        std::thread::Builder::new().name("usb-hid".into()).spawn(move || { let _ = tx.send(set_hid(on)); }).ok();
        HidJob { on, rx, started: Instant::now() }
    }
    pub fn poll(&self) -> Option<Result<String, String>> {
        match self.rx.try_recv() {
            Ok(r) => Some(r),
            Err(_) if self.started.elapsed() > Duration::from_secs(15) => Some(Err("timed out".into())),
            Err(_) => None,
        }
    }
}


/// Switch now (blocking, up to ~10 s). Ok(new role word) or Err(reason).
pub fn set(to: UsbTarget, force: bool, persist: bool) -> Result<String, String> {
    if sys::sim() {
        std::thread::sleep(Duration::from_millis(400));
        let v = std::env::var("ZPOD_SIM_USB_SWITCH").unwrap_or_else(|_| "ok".into());
        if v != "ok" {
            return Err(v);
        }
        let r = match to {
            UsbTarget::Host => "host",
            UsbTarget::Gadget => "gadget",
            UsbTarget::Auto => "otg",
        };
        std::env::set_var("ZPOD_USB_ROLE", r);
        if persist {
            std::env::set_var("ZPOD_SIM_USB_NEXT", if to == UsbTarget::Host { "host" } else { "gadget" });
        }
        return Ok(r.into());
    }
    let t = tool().ok_or("this image has no zpod-usb-role (update the image or run scripts/zpod-dev --core)")?;
    let mut c = Command::new(&t);
    c.args(["set", to.word()]);
    if force {
        c.arg("--force");
    }
    if persist {
        c.arg("--persist");
    }
    let o = c.output().map_err(|e| format!("{}: {e}", t.display()))?;
    let out = String::from_utf8_lossy(&o.stdout);
    let last = out.lines().filter(|l| l.starts_with("ok") || l.starts_with("err")).last().unwrap_or("").trim().to_string();
    eprintln!("launcher: usb {}{}{}: {}", to.word(), if force { " force" } else { "" }, if persist { " persist" } else { "" }, if last.is_empty() { "no answer" } else { &last });
    if let Some(r) = last.strip_prefix("ok") {
        Ok(r.trim().to_string())
    } else if let Some(r) = last.strip_prefix("err") {
        Err(r.trim().to_string())
    } else {
        Err(format!("zpod-usb-role exited {}", o.status.code().unwrap_or(-1)))
    }
}

/// A switch running on a thread (Tools > USB).
pub struct Job {
    pub to: UsbTarget,
    pub started: Instant,
    rx: Receiver<Result<String, String>>,
}

impl Job {
    pub fn start(to: UsbTarget, force: bool, persist: bool) -> Job {
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(set(to, force, persist));
        });
        Job { to, started: Instant::now(), rx }
    }

    pub fn poll(&self) -> Option<Result<String, String>> {
        match self.rx.try_recv() {
            Ok(r) => Some(r),
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err("switch thread failed".into())),
        }
    }
}

/// What a target means on screen.
pub fn target_label(t: UsbTarget) -> &'static str {
    match t {
        UsbTarget::Host => "USB devices (host)",
        UsbTarget::Gadget => "Computer link (gadget)",
        UsbTarget::Auto => "Auto: follow the cable",
    }
}

pub fn role_label(r: Option<UsbRole>) -> &'static str {
    match r {
        Some(UsbRole::Host) => "USB devices",
        Some(UsbRole::Gadget) => "Computer link",
        Some(UsbRole::Otg) => "Auto (OTG)",
        _ => "Unknown",
    }
}

pub fn is_current(t: UsbTarget, r: Option<UsbRole>) -> bool {
    matches!((t, r), (UsbTarget::Host, Some(UsbRole::Host)) | (UsbTarget::Gadget, Some(UsbRole::Gadget)) | (UsbTarget::Auto, Some(UsbRole::Otg)))
}

/// Can `--persist` be offered for this target?
pub fn can_persist(i: &RoleInfo, t: UsbTarget) -> bool {
    t != UsbTarget::Auto && (i.persist == "file" || i.persist == "config")
}

/// Is `pid` the app `root` or one of its children? (/proc/<pid>/stat ppid)
pub fn is_descendant(pid: u32, root: u32) -> bool {
    let mut p = pid;
    for _ in 0..32 {
        if p == root {
            return true;
        }
        if p <= 1 {
            return false;
        }
        match std::fs::read_to_string(format!("/proc/{p}/stat")).ok().and_then(|s| ppid_of(&s)) {
            Some(pp) => p = pp,
            None => return false,
        }
    }
    false
}

/// Field 4 of /proc/<pid>/stat (after the ")" that ends the name).
pub fn ppid_of(stat: &str) -> Option<u32> {
    let rest = &stat[stat.rfind(')')? + 1..];
    rest.split_whitespace().nth(1)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stat_parent() {
        assert_eq!(ppid_of("1234 (gps) S 279 1234 1234 0 -1"), Some(279));
        assert_eq!(ppid_of("99 (we ird) name)) R 7 99"), Some(7));
        assert_eq!(ppid_of("garbage"), None);
        let me = std::process::id();
        assert!(is_descendant(me, me));
        if std::path::Path::new("/proc/self/stat").exists() {
            assert!(!is_descendant(me, u32::MAX - 1));
            let parent = ppid_of(&std::fs::read_to_string("/proc/self/stat").unwrap()).unwrap();
            assert!(is_descendant(me, parent));
        }
    }

    #[test]
    fn labels_and_persist() {
        assert!(is_current(UsbTarget::Host, Some(UsbRole::Host)));
        assert!(!is_current(UsbTarget::Host, Some(UsbRole::Gadget)));
        assert!(is_current(UsbTarget::Auto, Some(UsbRole::Otg)));
        let mut i = RoleInfo::parse("role=gadget persist=file");
        assert!(can_persist(&i, UsbTarget::Host) && !can_persist(&i, UsbTarget::Auto));
        i.persist = "none".into();
        assert!(!can_persist(&i, UsbTarget::Gadget));
    }
}

#[cfg(test)]
mod midi_tests {
    use super::Midi;

    #[test]
    fn midi_status_lines() {
        assert_eq!(Midi::parse("role=gadget\nmidi=on\nmidi_live=1\n"), Midi { known: true, on: true, live: true });
        assert_eq!(Midi::parse("role=gadget\nmidi=off\nmidi_live=0\n"), Midi { known: true, on: false, live: false });
        assert!(!Midi::parse("role=gadget\n").known);
    }
}
