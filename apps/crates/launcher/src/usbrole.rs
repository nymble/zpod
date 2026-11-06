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

/// The tool: `ZPOD_USB_ROLE_TOOL`, a newer copy pushed to /data by
/// `scripts/zpod-dev --core` (root-owned, not writable by others), else the
/// image's.
pub fn tool() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("ZPOD_USB_ROLE_TOOL") {
        return Some(p.into());
    }
    let dev = PathBuf::from("/data/zpod/core/zpod-usb-role");
    if trusted(&dev) {
        return Some(dev);
    }
    let img = PathBuf::from("/usr/sbin/zpod-usb-role");
    img.exists().then_some(img)
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
    if sys::sim() {
        return role::sim_info();
    }
    if let Some(t) = tool() {
        if let Ok(o) = Command::new(&t).arg("status").output() {
            if o.status.success() {
                let i = RoleInfo::parse(&String::from_utf8_lossy(&o.stdout));
                if i.role.is_some() {
                    return i;
                }
            }
        }
    }
    let r = role::usb_role();
    RoleInfo { role: Some(r), boot: Some(r), next: Some(r), persist: "none".into(), ..Default::default() }
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
