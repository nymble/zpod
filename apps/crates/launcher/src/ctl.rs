//! Launcher side of the control sockets (protocol: `zpod_apps::ctl`).
//!
//! A thread accepts connections on `/run/zpod/launcher.sock` (mode 0600, so
//! only root can drive it), parses one request line and hands it to the main
//! loop through a channel; the main loop (or the wait loop of a running app)
//! answers. Requests are handled one at a time.
//!
//! A second thread serves `/run/zpod/app.sock` (mode 0666) for apps: only
//! `usb` requests are accepted there, tagged with the caller's pid
//! (SO_PEERCRED) so the launcher can check it is the app on the screen.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;
use zpod_apps::ctl::{Request, MAX_LINE};

pub struct Pending {
    pub req: Request,
    reply: Sender<String>,
    /// came in on the app socket
    pub from_app: bool,
    /// caller's pid (Linux SO_PEERCRED)
    pub peer_pid: Option<u32>,
}

impl Pending {
    pub fn reply(self, line: impl Into<String>) {
        let _ = self.reply.send(line.into());
    }
}

/// Root socket (0600) + app socket (0666, only `allowed_for_apps`)
/// sharing one channel; `None` (and a log line) if the root socket cannot
/// be created.
pub fn start_both(root: &Path, app: &Path) -> Option<Receiver<Pending>> {
    let (tx, rx) = channel();
    listen(root, false, tx.clone())?;
    if listen(app, true, tx).is_none() {
        eprintln!("launcher: no app socket; apps cannot ask for USB host mode");
    }
    Some(rx)
}

fn listen(path: &Path, app: bool, tx: Sender<Pending>) -> Option<()> {
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if std::fs::symlink_metadata(path).map(|m| m.file_type().is_socket()).unwrap_or(false) {
        let _ = std::fs::remove_file(path); // stale, from the previous launcher
    }
    let l = match UnixListener::bind(path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("launcher: control socket {}: {e}", path.display());
            return None;
        }
    };
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(if app { 0o666 } else { 0o600 }));
    std::thread::Builder::new()
        .name(if app { "ctl-app" } else { "ctl" }.into())
        .spawn(move || {
            for s in l.incoming().flatten() {
                serve(s, &tx, app);
            }
        })
        .ok()?;
    eprintln!("launcher: {} socket {}", if app { "app" } else { "control" }, path.display());
    Some(())
}

#[cfg(target_os = "linux")]
fn peer_pid(s: &UnixStream) -> Option<u32> {
    use std::os::fd::AsRawFd;
    let mut c = libc::ucred { pid: 0, uid: 0, gid: 0 };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let r = unsafe { libc::getsockopt(s.as_raw_fd(), libc::SOL_SOCKET, libc::SO_PEERCRED, &mut c as *mut _ as *mut libc::c_void, &mut len) };
    (r == 0 && c.pid > 0).then_some(c.pid as u32)
}

#[cfg(not(target_os = "linux"))]
fn peer_pid(_: &UnixStream) -> Option<u32> {
    None
}

fn serve(s: UnixStream, tx: &Sender<Pending>, app: bool) {
    let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = s.set_write_timeout(Some(Duration::from_secs(2)));
    // (the client waits for the reply with its own timeout)
    let mut line = String::new();
    if BufReader::new(&s).take(MAX_LINE as u64).read_line(&mut line).is_err() {
        return;
    }
    let reply = match Request::parse(&line) {
        Err(e) => format!("err {e}"),
        Ok(req) if app && !req.allowed_for_apps() => "err not allowed from apps (usb status | usb host|gadget|auto [force])".to_string(),
        Ok(req) => {
            let (rtx, rrx) = channel();
            let peer = if app { peer_pid(&s) } else { None };
            if tx.send(Pending { req, reply: rtx, from_app: app, peer_pid: peer }).is_err() {
                return;
            }
            // Stopping an app waits up to its grace period (1.5 s) plus a
            // relaunch; a USB switch takes up to ~10 s.
            rrx.recv_timeout(Duration::from_secs(20)).unwrap_or_else(|_| "err launcher busy".into())
        }
    };
    let _ = (&s).write_all(format!("{reply}\n").as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use zpod_apps::ctl::{send_to, UsbReq, UsbTarget};

    #[test]
    fn app_socket_only_passes_usb_requests() {
        let d = std::env::temp_dir().join(format!("zpod-ctl-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&d);
        let (root, app) = (d.join("launcher.sock"), d.join("app.sock"));
        let rx = start_both(&root, &app).expect("sockets");
        assert_eq!(std::fs::metadata(&app).unwrap().permissions().mode() & 0o777, 0o666);
        assert_eq!(std::fs::metadata(&root).unwrap().permissions().mode() & 0o777, 0o600);
        let t = std::thread::spawn(move || {
            for _ in 0..2 {
                let p = rx.recv_timeout(Duration::from_secs(5)).unwrap();
                let who = if p.from_app { "app" } else { "root" };
                #[cfg(target_os = "linux")]
                if p.from_app {
                    assert_eq!(p.peer_pid, Some(std::process::id()));
                }
                p.reply(format!("ok {who}"));
            }
        });
        let to = Duration::from_secs(5);
        let set = Request::Usb(UsbReq::Set { to: UsbTarget::Host, force: false, persist: false });
        // not allowed on the app socket: answered there, never reaches the launcher
        let r = send_to(&app, &Request::Status, to).unwrap_err();
        assert!(r.starts_with("not allowed"), "{r}");
        assert_eq!(send_to(&app, &set, to).unwrap(), "app");
        assert_eq!(send_to(&root, &Request::Status, to).unwrap(), "root");
        t.join().unwrap();
        let _ = std::fs::remove_dir_all(&d);
    }
}
