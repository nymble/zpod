//! Process plumbing: signals, boot-splash hand-off, running apps as an
//! unprivileged user, power actions. Works with or without systemd.

use std::ffi::CString;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub static TERM: AtomicBool = AtomicBool::new(false);

extern "C" fn on_term(_: libc::c_int) {
    TERM.store(true, Ordering::SeqCst);
}

pub fn install_signal_handlers() {
    unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = on_term as *const () as usize;
        libc::sigemptyset(&mut sa.sa_mask);
        sa.sa_flags = 0; // no SA_RESTART: blocking poll() returns EINTR promptly
        libc::sigaction(libc::SIGTERM, &sa, std::ptr::null_mut());
        libc::sigaction(libc::SIGINT, &sa, std::ptr::null_mut());
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }
}

pub fn terminating() -> bool {
    TERM.load(Ordering::SeqCst)
}

pub const SPLASH_PID: &str = "/run/zpod/bootsplash.pid";

fn alive(pid: i32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
        && std::fs::read_to_string(format!("/proc/{pid}/stat")).map(|s| !s.contains(") Z ")).unwrap_or(false)
}

/// Ask a running zpod-bootsplash to exit (it leaves its last frame on the
/// panel and the init marker in place, so we take over with a warm init and
/// no flash). Under systemd `Conflicts=` does the same; this covers BusyBox
/// init or a manual start.
pub fn stop_splash() {
    let Some(pid) = std::fs::read_to_string(SPLASH_PID).ok().and_then(|s| s.trim().parse::<i32>().ok()) else { return };
    if pid <= 1 || !alive(pid) {
        return;
    }
    let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default();
    if !comm.trim().starts_with("zpod-bootsplash") {
        return; // stale pid file
    }
    unsafe { libc::kill(pid, libc::SIGTERM) };
    let until = Instant::now() + Duration::from_millis(1500);
    while alive(pid) && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Unprivileged account apps run as (only when the launcher is root).
pub struct AppUser {
    pub uid: u32,
    pub gid: u32,
    pub name: String,
    pub home: String,
    groups: Vec<libc::gid_t>,
}

pub fn lookup_user(name: &str) -> Option<AppUser> {
    let cname = CString::new(name).ok()?;
    let pw = unsafe { libc::getpwnam(cname.as_ptr()) };
    if pw.is_null() {
        return None;
    }
    let pw = unsafe { &*pw };
    let home = unsafe { std::ffi::CStr::from_ptr(pw.pw_dir) }.to_string_lossy().into_owned();
    let mut n: libc::c_int = 64;
    let mut groups = vec![0 as libc::gid_t; 64];
    let r = unsafe { libc::getgrouplist(cname.as_ptr(), pw.pw_gid, groups.as_mut_ptr(), &mut n) };
    groups.truncate(if r < 0 { 0 } else { n as usize });
    Some(AppUser { uid: pw.pw_uid, gid: pw.pw_gid, name: name.to_string(), home, groups })
}

pub fn spawn_app(exec: &Path, args: &[String], dir: &Path, env: &[(String, String)], user: Option<&AppUser>) -> std::io::Result<Child> {
    let mut c = Command::new(exec);
    c.args(args).current_dir(dir).process_group(0);
    for (k, v) in env {
        c.env(k, v);
    }
    if let Some(u) = user {
        c.env("HOME", &u.home).env("USER", &u.name).env("LOGNAME", &u.name);
        let (uid, gid, groups) = (u.uid, u.gid, u.groups.clone());
        unsafe {
            c.pre_exec(move || {
                if libc::setgroups(groups.len() as _, groups.as_ptr()) != 0
                    || libc::setgid(gid) != 0
                    || libc::setuid(uid) != 0
                {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    c.spawn()
}

/// Signal the app's whole process group.
pub fn signal_group(child: &Child, sig: libc::c_int) {
    unsafe { libc::kill(-(child.id() as i32), sig) };
}

/// SIGTERM, wait up to `grace`, then SIGKILL. Returns once it is reaped.
pub fn stop_child(child: &mut Child, grace: Duration) {
    signal_group(child, libc::SIGTERM);
    let until = Instant::now() + grace;
    while Instant::now() < until {
        if let Ok(Some(_)) = child.try_wait() {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    signal_group(child, libc::SIGKILL);
    let _ = child.wait();
}

pub fn have_systemd() -> bool {
    Path::new("/run/systemd/system").exists()
}

pub const STOP_KIND: &str = "/run/zpod/stop-kind";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopKind {
    Poweroff,
    Reboot,
}

/// Why are we being stopped? Our own marker first (written by our power
/// menu, or by a BusyBox shutdown script: `echo poweroff > /run/zpod/stop-kind`),
/// then systemd's job queue (same test as the M3 Python UI).
pub fn stop_kind() -> Option<StopKind> {
    if let Ok(s) = std::fs::read_to_string(STOP_KIND) {
        match s.trim() {
            "poweroff" | "halt" => return Some(StopKind::Poweroff),
            "reboot" => return Some(StopKind::Reboot),
            _ => {}
        }
    }
    if !have_systemd() {
        return None;
    }
    let out = Command::new("systemctl").args(["list-jobs", "--no-legend", "--plain"]).output().ok()?;
    let jobs = String::from_utf8_lossy(&out.stdout);
    if jobs.contains("reboot.target") || jobs.contains("kexec.target") {
        Some(StopKind::Reboot)
    } else if jobs.contains("poweroff.target") || jobs.contains("halt.target") {
        Some(StopKind::Poweroff)
    } else {
        None
    }
}

pub fn power(kind: StopKind) {
    let _ = std::fs::create_dir_all("/run/zpod");
    let word = match kind {
        StopKind::Poweroff => "poweroff",
        StopKind::Reboot => "reboot",
    };
    let _ = std::fs::write(STOP_KIND, word);
    let ok = if have_systemd() {
        Command::new("systemctl").arg(word).status().map(|s| s.success()).unwrap_or(false)
    } else {
        false
    };
    if !ok {
        let _ = Command::new(word).status(); // BusyBox / sysvinit
    }
}

/// [shutdown] screen = message | off   from /etc/zpod/ui.conf (default message).
pub fn shutdown_screen_mode(conf: &str) -> &'static str {
    let mut section = String::new();
    let mut mode = "message";
    for line in conf.lines() {
        let l = line.split('#').next().unwrap_or("").trim();
        if l.starts_with('[') && l.ends_with(']') {
            section = l[1..l.len() - 1].trim().to_lowercase();
        } else if section == "shutdown" {
            if let Some((k, v)) = l.split_once('=') {
                if k.trim() == "screen" {
                    mode = if v.trim() == "off" { "off" } else { "message" };
                }
            }
        }
    }
    mode
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ui_conf_mode() {
        assert_eq!(shutdown_screen_mode("[shutdown]\nscreen = off\n"), "off");
        assert_eq!(shutdown_screen_mode("[shutdown]\n# screen = off\nscreen = message"), "message");
        assert_eq!(shutdown_screen_mode("[other]\nscreen = off\n"), "message");
        assert_eq!(shutdown_screen_mode(""), "message");
    }
}
