//! Launcher control socket (`/run/zpod/launcher.sock`, root only, mode 0600).
//!
//! One request line per connection, one reply line back:
//!
//! | request            | reply                                             |
//! |--------------------|---------------------------------------------------|
//! | `launch <id>`      | `ok launching <id>` (stops any other running app) |
//! | `restart [<id>]`   | `ok restarting <id>` (stop, then start again)     |
//! | `stop [<id>]`      | `ok stopped <id>`, or `ok idle`                   |
//! | `status`           | `ok running <id> <pid> <seconds>`, or `ok idle`   |
//! | `version`          | `ok zpod-launcher <version> ctl <protocol>`       |
//!
//! Errors are `err <reason>`. The launcher is the only LCD owner, so this is
//! how the Mac (over ssh) starts, restarts and stops apps without fighting it
//! for the panel. `zpod-app launch|restart|stop|status` are the clients.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::time::Duration;

pub const DEFAULT_SOCKET: &str = "/run/zpod/launcher.sock";
pub const PROTOCOL: u32 = 1;
/// Longest request line accepted.
pub const MAX_LINE: usize = 256;

pub fn socket_path() -> PathBuf {
    std::env::var_os("ZPOD_SOCKET").map(Into::into).unwrap_or_else(|| DEFAULT_SOCKET.into())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    Launch(String),
    Restart(Option<String>),
    Stop(Option<String>),
    Status,
    Version,
}

fn id_arg(s: Option<&str>) -> Result<Option<String>, String> {
    match s {
        None => Ok(None),
        Some(id) if crate::manifest::valid_id(id) => Ok(Some(id.to_string())),
        Some(id) => Err(format!("invalid app id '{id}'")),
    }
}

impl Request {
    pub fn parse(line: &str) -> Result<Request, String> {
        let mut w = line.split_whitespace();
        let cmd = w.next().unwrap_or("");
        let arg = w.next();
        if w.next().is_some() {
            return Err("too many arguments".into());
        }
        match cmd {
            "launch" => id_arg(arg)?.map(Request::Launch).ok_or_else(|| "launch needs an app id".into()),
            "restart" => Ok(Request::Restart(id_arg(arg)?)),
            "stop" => Ok(Request::Stop(id_arg(arg)?)),
            "status" if arg.is_none() => Ok(Request::Status),
            "version" if arg.is_none() => Ok(Request::Version),
            "" => Err("empty request".into()),
            c => Err(format!("unknown request '{c}'")),
        }
    }

    pub fn to_line(&self) -> String {
        match self {
            Request::Launch(id) => format!("launch {id}"),
            Request::Restart(Some(id)) => format!("restart {id}"),
            Request::Restart(None) => "restart".into(),
            Request::Stop(Some(id)) => format!("stop {id}"),
            Request::Stop(None) => "stop".into(),
            Request::Status => "status".into(),
            Request::Version => "version".into(),
        }
    }
}

/// Parsed `status` reply.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Idle,
    Running { id: String, pid: u32, secs: u64 },
}

impl Status {
    pub fn parse(reply: &str) -> Option<Status> {
        let mut w = reply.split_whitespace();
        match (w.next(), w.next()) {
            (Some("ok"), Some("idle")) => Some(Status::Idle),
            (Some("ok"), Some("running")) => {
                let id = w.next()?.to_string();
                let pid = w.next()?.parse().ok()?;
                let secs = w.next()?.parse().ok()?;
                Some(Status::Running { id, pid, secs })
            }
            _ => None,
        }
    }

    pub fn reply(&self) -> String {
        match self {
            Status::Idle => "ok idle".into(),
            Status::Running { id, pid, secs } => format!("ok running {id} {pid} {secs}"),
        }
    }
}

/// Send one request; returns the reply text after `ok `, or the reason
/// after `err ` as an error. `Err` also when the launcher is not listening
/// (no socket: the image's launcher predates the control socket).
pub fn send(req: &Request, timeout: Duration) -> Result<String, String> {
    let path = socket_path();
    let mut s = std::os::unix::net::UnixStream::connect(&path).map_err(|e| {
        format!("launcher control socket {}: {e} (launcher too old or not running? see docs/DEV-LOOP.md)", path.display())
    })?;
    s.set_read_timeout(Some(timeout)).ok();
    s.set_write_timeout(Some(timeout)).ok();
    writeln!(s, "{}", req.to_line()).map_err(|e| e.to_string())?;
    let mut line = String::new();
    BufReader::new(s).take(4096).read_line(&mut line).map_err(|e| format!("no reply from launcher: {e}"))?;
    let line = line.trim();
    if let Some(r) = line.strip_prefix("ok") {
        Ok(r.trim().to_string())
    } else if let Some(r) = line.strip_prefix("err") {
        Err(r.trim().to_string())
    } else {
        Err(format!("bad reply from launcher: '{line}'"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_requests() {
        assert_eq!(Request::parse("launch clock").unwrap(), Request::Launch("clock".into()));
        assert_eq!(Request::parse("  restart  ").unwrap(), Request::Restart(None));
        assert_eq!(Request::parse("restart gen-flow\n").unwrap(), Request::Restart(Some("gen-flow".into())));
        assert_eq!(Request::parse("stop").unwrap(), Request::Stop(None));
        assert_eq!(Request::parse("status").unwrap(), Request::Status);
        assert_eq!(Request::parse("version").unwrap(), Request::Version);
        assert!(Request::parse("launch").is_err());
        assert!(Request::parse("launch ../x").is_err());
        assert!(Request::parse("launch Clock").is_err());
        assert!(Request::parse("status x").is_err());
        assert!(Request::parse("rm -rf").is_err());
        assert!(Request::parse("").is_err());
        assert!(Request::parse("launch a b").is_err());
        for r in [Request::Launch("a".into()), Request::Restart(None), Request::Restart(Some("b".into())), Request::Stop(None), Request::Status, Request::Version] {
            assert_eq!(Request::parse(&r.to_line()).unwrap(), r);
        }
    }

    #[test]
    fn status_round_trip() {
        for s in [Status::Idle, Status::Running { id: "clock".into(), pid: 42, secs: 7 }] {
            assert_eq!(Status::parse(&s.reply()), Some(s.clone()));
        }
        assert_eq!(Status::parse("err nope"), None);
    }

    #[test]
    fn client_round_trip() {
        let path = std::env::temp_dir().join(format!("zpod-ctl-test-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let l = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let t = std::thread::spawn(move || {
            for _ in 0..2 {
                let (s, _) = l.accept().unwrap();
                let mut line = String::new();
                BufReader::new(&s).read_line(&mut line).unwrap();
                let reply = match Request::parse(&line) {
                    Ok(Request::Status) => "ok idle".to_string(),
                    Ok(r) => format!("err cannot {}", r.to_line()),
                    Err(e) => format!("err {e}"),
                };
                (&s).write_all(format!("{reply}\n").as_bytes()).unwrap();
            }
        });
        std::env::set_var("ZPOD_SOCKET", &path);
        assert_eq!(send(&Request::Status, Duration::from_secs(2)).unwrap(), "idle");
        assert_eq!(send(&Request::Stop(None), Duration::from_secs(2)).unwrap_err(), "cannot stop");
        std::env::remove_var("ZPOD_SOCKET");
        t.join().unwrap();
        let _ = std::fs::remove_file(&path);
    }
}
