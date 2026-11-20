use std::collections::HashSet;
use std::io::{BufRead, BufReader, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use zpod_labd::proto::{self, Cmd};
use zpod_labd::{SOCKET, VERSION};

const AF_BLUETOOTH: libc::c_int = 31;
const BTPROTO_HCI: libc::c_int = 1;
const HCI_CHANNEL_RAW: u16 = 0;
const HCI_CHANNEL_MONITOR: u16 = 2;
const HCI_DEV_NONE: u16 = 0xffff;

#[repr(C)]
struct SockaddrHci {
    family: libc::sa_family_t,
    dev: u16,
    channel: u16,
}

type Busy = Arc<Mutex<HashSet<String>>>;

pub fn main() {
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }
    let _ = std::fs::remove_file(SOCKET);
    let l = match UnixListener::bind(SOCKET) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("zpod-labd: bind {SOCKET}: {e}");
            std::process::exit(1);
        }
    };
    // anyone may connect; SO_PEERCRED decides (root or the app user)
    let _ = std::fs::set_permissions(SOCKET, std::fs::Permissions::from_mode(0o666));
    let app_uid = app_uid();
    eprintln!("zpod-labd {VERSION}: listening on {SOCKET} (app uid {app_uid})");
    let busy: Busy = Arc::new(Mutex::new(HashSet::new()));
    for c in l.incoming().flatten() {
        let uid = peer_uid(&c);
        if !uid.is_some_and(|u| proto::peer_allowed(u, app_uid)) {
            let mut c = c;
            let _ = c.write_all(b"err not allowed\n");
            continue;
        }
        let busy = busy.clone();
        let _ = std::thread::Builder::new().name("labd-conn".into()).spawn(move || handle(c, busy));
    }
}

fn app_uid() -> u32 {
    std::fs::read_to_string("/etc/passwd")
        .ok()
        .and_then(|p| p.lines().find(|l| l.starts_with("zpod:")).and_then(|l| l.split(':').nth(2)).and_then(|u| u.parse().ok()))
        .unwrap_or(1000)
}

fn peer_uid(s: &UnixStream) -> Option<u32> {
    let mut c = libc::ucred { pid: 0, uid: 0, gid: 0 };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: getsockopt into a correctly sized ucred
    let r = unsafe { libc::getsockopt(s.as_raw_fd(), libc::SOL_SOCKET, libc::SO_PEERCRED, (&mut c as *mut libc::ucred).cast(), &mut len) };
    (r == 0).then_some(c.uid)
}

fn run(cmd: &[&str]) -> Result<(), String> {
    let o = Command::new(cmd[0]).args(&cmd[1..]).output().map_err(|e| format!("{}: {e}", cmd[0]))?;
    if o.status.success() {
        Ok(())
    } else {
        Err(format!("{} failed: {}", cmd.join(" "), String::from_utf8_lossy(&o.stderr).trim()))
    }
}

fn target(iface: &str) -> Result<zpod_wlan::Adapter, String> {
    let v = zpod_wlan::inventory();
    proto::wlan_target(iface, &v).cloned()
}

fn set_monitor(iface: &str, chan: u32) -> Result<(), String> {
    target(iface)?;
    run(&["ip", "link", "set", iface, "down"])?;
    run(&["iw", "dev", iface, "set", "type", "monitor"])?;
    run(&["ip", "link", "set", iface, "up"])?;
    run(&["iw", "dev", iface, "set", "channel", &chan.to_string()])
}

fn set_managed(iface: &str) -> Result<(), String> {
    target(iface)?;
    run(&["ip", "link", "set", iface, "down"])?;
    run(&["iw", "dev", iface, "set", "type", "managed"])
}

fn handle(c: UnixStream, busy: Busy) {
    let mut r = BufReader::new(match c.try_clone() {
        Ok(x) => x,
        Err(_) => return,
    });
    let mut w = c;
    let _ = w.set_read_timeout(Some(Duration::from_secs(10)));
    let mut line = String::new();
    if r.read_line(&mut line).is_err() {
        return;
    }
    let cmd = match proto::parse(&line) {
        Ok(c) => c,
        Err(e) => {
            let _ = writeln!(w, "err {e}");
            return;
        }
    };
    eprintln!("zpod-labd: {cmd:?}");
    let res = match cmd {
        Cmd::Ping => Ok(format!("zpod-labd {VERSION}")),
        Cmd::WlanMonitor { iface, chan } => set_monitor(&iface, chan).map(|_| format!("{iface} monitor ch {chan}")),
        Cmd::WlanChan { iface, chan } => target(&iface).and_then(|a| {
            if !a.in_monitor() {
                return Err(format!("{iface} is not in monitor mode"));
            }
            run(&["iw", "dev", &iface, "set", "channel", &chan.to_string()]).map(|_| format!("ch {chan}"))
        }),
        Cmd::WlanManaged { iface } => set_managed(&iface).map(|_| format!("{iface} managed")),
        Cmd::WlanStream { iface } => {
            stream_wlan(&iface, w, &busy);
            return;
        }
        Cmd::HciStream => {
            stream_hci(w, &busy);
            return;
        }
    };
    let _ = match res {
        Ok(m) => writeln!(w, "ok {m}"),
        Err(e) => writeln!(w, "err {e}"),
    };
}

/// Mark `key` busy for the guard's life.
struct Claim(Busy, String);
impl Drop for Claim {
    fn drop(&mut self) {
        self.0.lock().unwrap().remove(&self.1);
    }
}
fn claim(busy: &Busy, key: &str) -> Option<Claim> {
    busy.lock().unwrap().insert(key.to_string()).then(|| Claim(busy.clone(), key.to_string()))
}

fn sock(domain: libc::c_int, ty: libc::c_int, proto: libc::c_int) -> Result<OwnedFd, String> {
    // SAFETY: plain socket(2)
    let fd = unsafe { libc::socket(domain, ty | libc::SOCK_CLOEXEC, proto) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    // SAFETY: fresh fd we own
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    let tv = libc::timeval { tv_sec: 1, tv_usec: 0 };
    // SAFETY: setsockopt with a timeval
    unsafe { libc::setsockopt(fd.as_raw_fd(), libc::SOL_SOCKET, libc::SO_RCVTIMEO, (&tv as *const libc::timeval).cast(), std::mem::size_of::<libc::timeval>() as libc::socklen_t) };
    Ok(fd)
}

/// Copy packets from `fd` to the client until it hangs up.
fn pump(fd: &OwnedFd, mut w: UnixStream) {
    let _ = w.set_write_timeout(Some(Duration::from_secs(2)));
    let mut buf = vec![0u8; 8192];
    let mut last = Instant::now();
    loop {
        // SAFETY: recv into our buffer
        let n = unsafe { libc::recv(fd.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len(), 0) };
        let res = if n > 0 {
            let n = n as usize;
            last = Instant::now();
            w.write_all(&proto::frame_header(n)).and_then(|_| w.write_all(&buf[..n]))
        } else if last.elapsed() >= Duration::from_secs(1) {
            last = Instant::now();
            w.write_all(&proto::frame_header(0))
        } else {
            Ok(())
        };
        if res.is_err() {
            return;
        }
        if n < 0 {
            let e = std::io::Error::last_os_error();
            if !matches!(e.raw_os_error(), Some(libc::EAGAIN) | Some(libc::EINTR)) {
                eprintln!("zpod-labd: recv: {e}");
                return;
            }
        }
    }
}

fn stream_wlan(iface: &str, mut w: UnixStream, busy: &Busy) {
    let Some(_claim) = claim(busy, &format!("wlan:{iface}")) else {
        let _ = writeln!(w, "err {iface} is already streaming");
        return;
    };
    let ready = (|| -> Result<OwnedFd, String> {
        let a = target(iface)?;
        if !a.in_monitor() {
            return Err(format!("{iface} is not in monitor mode"));
        }
        let idx = unsafe { libc::if_nametoindex(std::ffi::CString::new(iface).unwrap().as_ptr()) };
        if idx == 0 {
            return Err(format!("{iface}: no index"));
        }
        let proto_all = (libc::ETH_P_ALL as u16).to_be() as libc::c_int;
        let fd = sock(libc::AF_PACKET, libc::SOCK_RAW, proto_all)?;
        let mut sa: libc::sockaddr_ll = unsafe { std::mem::zeroed() };
        sa.sll_family = libc::AF_PACKET as u16;
        sa.sll_protocol = (libc::ETH_P_ALL as u16).to_be();
        sa.sll_ifindex = idx as i32;
        // SAFETY: bind with a sockaddr_ll
        let r = unsafe { libc::bind(fd.as_raw_fd(), (&sa as *const libc::sockaddr_ll).cast(), std::mem::size_of::<libc::sockaddr_ll>() as libc::socklen_t) };
        if r != 0 {
            return Err(format!("bind {iface}: {}", std::io::Error::last_os_error()));
        }
        Ok(fd)
    })();
    match ready {
        Ok(fd) => {
            if writeln!(w, "ok stream {iface}").is_ok() {
                pump(&fd, w);
            }
            drop(fd);
            // leave the adapter as we found it before the lab
            match set_managed(iface) {
                Ok(()) => eprintln!("zpod-labd: {iface} stream ended, back to managed"),
                Err(e) => eprintln!("zpod-labd: {iface} restore: {e}"),
            }
        }
        Err(e) => {
            let _ = writeln!(w, "err {e}");
        }
    }
}

fn hci_cmd(pkts: &[&[u8]]) -> Result<(), String> {
    let fd = sock(AF_BLUETOOTH, libc::SOCK_RAW, BTPROTO_HCI)?;
    let sa = SockaddrHci { family: AF_BLUETOOTH as libc::sa_family_t, dev: 0, channel: HCI_CHANNEL_RAW };
    // SAFETY: bind with a sockaddr_hci
    if unsafe { libc::bind(fd.as_raw_fd(), (&sa as *const SockaddrHci).cast(), std::mem::size_of::<SockaddrHci>() as libc::socklen_t) } != 0 {
        return Err(format!("hci0: {}", std::io::Error::last_os_error()));
    }
    for p in pkts {
        // SAFETY: write a complete HCI command packet
        let n = unsafe { libc::write(fd.as_raw_fd(), p.as_ptr().cast(), p.len()) };
        if n != p.len() as isize {
            return Err(format!("hci0 command: {}", std::io::Error::last_os_error()));
        }
        std::thread::sleep(Duration::from_millis(60));
    }
    Ok(())
}

fn stream_hci(mut w: UnixStream, busy: &Busy) {
    let Some(_claim) = claim(busy, "hci") else {
        let _ = writeln!(w, "err BLE capture already running");
        return;
    };
    let ready = (|| -> Result<OwnedFd, String> {
        let fd = sock(AF_BLUETOOTH, libc::SOCK_RAW, BTPROTO_HCI)?;
        let sa = SockaddrHci { family: AF_BLUETOOTH as libc::sa_family_t, dev: HCI_DEV_NONE, channel: HCI_CHANNEL_MONITOR };
        // SAFETY: bind with a sockaddr_hci
        if unsafe { libc::bind(fd.as_raw_fd(), (&sa as *const SockaddrHci).cast(), std::mem::size_of::<SockaddrHci>() as libc::socklen_t) } != 0 {
            return Err(format!("HCI monitor: {} (Bluetooth driver loaded?)", std::io::Error::last_os_error()));
        }
        // controller on (no advertising, not discoverable), then passive scan
        let _ = run(&["btmgmt", "--index", "0", "power", "on"]);
        hci_cmd(&[&proto::hci_scan_enable(false), &proto::hci_scan_params_passive(), &proto::hci_scan_enable(true)])?;
        Ok(fd)
    })();
    match ready {
        Ok(fd) => {
            if writeln!(w, "ok stream hci0 passive").is_ok() {
                pump(&fd, w);
            }
            let _ = hci_cmd(&[&proto::hci_scan_enable(false)]);
            eprintln!("zpod-labd: BLE stream ended, scan off");
        }
        Err(e) => {
            let _ = hci_cmd(&[&proto::hci_scan_enable(false)]);
            let _ = writeln!(w, "err {e}");
        }
    }
}
