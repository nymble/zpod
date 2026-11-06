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
//! | `usb status`       | `ok role=gadget boot=gadget safe=1 ...` (key=value) |
//! | `usb host\|gadget\|auto [force] [persist]` | `ok host` after a live switch (zpod-usb-role) |
//!
//! Apps (user `zpod`) cannot open this socket. They use the app socket
//! (`/run/zpod/app.sock`, [`APP_SOCKET`]), which takes only the `usb`
//! requests: `usb status` from any app, `usb host|gadget|auto [force]` only
//! from the app on the screen and only if its manifest lists
//! `permissions = ["usb-host"]` (the launcher checks the caller's pid).
//! The app asks the user first (Switch USB? dialog); `persist` (next boot
//! too) is root-socket only.
//!
//! Errors are `err <reason>`. The launcher is the only LCD owner, so this is
//! how the Mac (over ssh) starts, restarts and stops apps without fighting it
//! for the panel. `zpod-app launch|restart|stop|status` are the clients.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::time::Duration;

pub const DEFAULT_SOCKET: &str = "/run/zpod/launcher.sock";
/// Requests from apps (mode 0666; the launcher authorises by peer pid).
pub const APP_SOCKET: &str = "/run/zpod/app.sock";
/// 2: `usb` requests and the app socket.
pub const PROTOCOL: u32 = 2;
/// Longest request line accepted.
pub const MAX_LINE: usize = 256;

pub fn socket_path() -> PathBuf {
    std::env::var_os("ZPOD_SOCKET").map(Into::into).unwrap_or_else(|| DEFAULT_SOCKET.into())
}

pub fn app_socket_path() -> PathBuf {
    std::env::var_os("ZPOD_APP_SOCKET").map(Into::into).unwrap_or_else(|| APP_SOCKET.into())
}

/// Role asked for in a `usb` request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UsbTarget {
    /// keyboard / mouse / GPS (dwc2 host)
    Host,
    /// link to the computer (dwc2 peripheral + CDC-NCM gadget)
    Gadget,
    /// follow the cable's ID pin (dwc2 otg), experimental
    Auto,
}

impl UsbTarget {
    pub fn word(self) -> &'static str {
        match self {
            UsbTarget::Host => "host",
            UsbTarget::Gadget => "gadget",
            UsbTarget::Auto => "auto",
        }
    }
    pub fn parse(s: &str) -> Option<UsbTarget> {
        match s {
            "host" => Some(UsbTarget::Host),
            "gadget" => Some(UsbTarget::Gadget),
            "auto" => Some(UsbTarget::Auto),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UsbReq {
    Status,
    Set { to: UsbTarget, force: bool, persist: bool },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    Launch(String),
    Restart(Option<String>),
    Stop(Option<String>),
    Status,
    Version,
    Usb(UsbReq),
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
        if cmd == "usb" {
            return Self::parse_usb(w.collect());
        }
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

    fn parse_usb(args: Vec<&str>) -> Result<Request, String> {
        match args.as_slice() {
            ["status"] => Ok(Request::Usb(UsbReq::Status)),
            [to, flags @ ..] => {
                let to = UsbTarget::parse(to).ok_or_else(|| format!("usb: unknown role '{to}' (host, gadget, auto, status)"))?;
                let (mut force, mut persist) = (false, false);
                for f in flags {
                    match *f {
                        "force" if !force => force = true,
                        "persist" if !persist => persist = true,
                        x => return Err(format!("usb: unknown option '{x}'")),
                    }
                }
                Ok(Request::Usb(UsbReq::Set { to, force, persist }))
            }
            [] => Err("usb needs status, host, gadget or auto".into()),
        }
    }

    /// Requests accepted on the app socket.
    pub fn allowed_for_apps(&self) -> bool {
        matches!(self, Request::Usb(UsbReq::Status) | Request::Usb(UsbReq::Set { persist: false, .. }))
    }

    pub fn to_line(&self) -> String {
        match self {
            Request::Usb(UsbReq::Status) => "usb status".into(),
            Request::Usb(UsbReq::Set { to, force, persist }) => {
                format!("usb {}{}{}", to.word(), if *force { " force" } else { "" }, if *persist { " persist" } else { "" })
            }
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
    send_to(&socket_path(), req, timeout)
}

/// [`send`] to another socket (the app socket).
pub fn send_to(path: &std::path::Path, req: &Request, timeout: Duration) -> Result<String, String> {
    let path = path.to_path_buf();
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
        assert_eq!(Request::parse("usb status").unwrap(), Request::Usb(UsbReq::Status));
        assert_eq!(Request::parse("usb host").unwrap(), Request::Usb(UsbReq::Set { to: UsbTarget::Host, force: false, persist: false }));
        assert_eq!(Request::parse("usb gadget persist force").unwrap(), Request::Usb(UsbReq::Set { to: UsbTarget::Gadget, force: true, persist: true }));
        assert!(Request::parse("usb").is_err());
        assert!(Request::parse("usb mass-storage").is_err());
        assert!(Request::parse("usb host now").is_err());
        assert!(Request::parse("usb host force force").is_err());
        assert!(Request::parse("usb status").unwrap().allowed_for_apps());
        assert!(Request::parse("usb host force").unwrap().allowed_for_apps());
        assert!(!Request::parse("usb host persist").unwrap().allowed_for_apps());
        assert!(!Request::parse("launch clock").unwrap().allowed_for_apps());
        assert!(!Request::parse("stop").unwrap().allowed_for_apps());
        for r in [
            Request::Launch("a".into()),
            Request::Restart(None),
            Request::Restart(Some("b".into())),
            Request::Stop(None),
            Request::Status,
            Request::Version,
            Request::Usb(UsbReq::Status),
            Request::Usb(UsbReq::Set { to: UsbTarget::Auto, force: true, persist: false }),
            Request::Usb(UsbReq::Set { to: UsbTarget::Gadget, force: false, persist: true }),
        ] {
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
