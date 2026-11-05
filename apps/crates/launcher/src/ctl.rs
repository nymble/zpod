//! Launcher side of the control socket (protocol: `zpod_apps::ctl`).
//!
//! A thread accepts connections on `/run/zpod/launcher.sock` (mode 0600, so
//! only root can drive it), parses one request line and hands it to the main
//! loop through a channel; the main loop (or the wait loop of a running app)
//! answers. Requests are handled one at a time.

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
}

impl Pending {
    pub fn reply(self, line: impl Into<String>) {
        let _ = self.reply.send(line.into());
    }
}

/// Start listening; `None` (and a log line) if the socket cannot be created.
pub fn start(path: &Path) -> Option<Receiver<Pending>> {
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
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    let (tx, rx) = channel();
    std::thread::Builder::new()
        .name("ctl".into())
        .spawn(move || {
            for s in l.incoming().flatten() {
                serve(s, &tx);
            }
        })
        .ok()?;
    eprintln!("launcher: control socket {}", path.display());
    Some(rx)
}

fn serve(s: UnixStream, tx: &Sender<Pending>) {
    let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = s.set_write_timeout(Some(Duration::from_secs(2)));
    let mut line = String::new();
    if BufReader::new(&s).take(MAX_LINE as u64).read_line(&mut line).is_err() {
        return;
    }
    let reply = match Request::parse(&line) {
        Err(e) => format!("err {e}"),
        Ok(req) => {
            let (rtx, rrx) = channel();
            if tx.send(Pending { req, reply: rtx }).is_err() {
                return;
            }
            // Stopping an app waits up to its grace period (1.5 s) plus a
            // relaunch; the main loop answers well inside this.
            rrx.recv_timeout(Duration::from_secs(12)).unwrap_or_else(|_| "err launcher busy".into())
        }
    };
    let _ = (&s).write_all(format!("{reply}\n").as_bytes());
}
