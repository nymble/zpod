//! USB serial GPS discovery (sysfs) and a 4800 8N1 raw line reader thread.
//!
//! Matching (spec §8.3): prefer ports bound to the `pl2303` driver, then
//! VID:PID 067b:2303 (a chip-class ID, not GlobalSat-unique), then any
//! other ttyUSB / ttyACM. Never guesses beyond what sysfs says.

use std::io::Read;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Port {
    pub dev: PathBuf,
    /// "ttyUSB0"
    pub name: String,
    pub driver: String,
    pub vid: u16,
    pub pid: u16,
    pub product: String,
}

impl Port {
    pub fn is_pl2303(&self) -> bool {
        self.driver == "pl2303" || (self.vid, self.pid) == (0x067b, 0x2303)
    }
    pub fn describe(&self) -> String {
        let p = if self.product.is_empty() { self.driver.clone() } else { self.product.clone() };
        format!("{} {:04x}:{:04x} {}", self.name, self.vid, self.pid, p)
    }
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).map(|s| s.trim().to_string()).unwrap_or_default()
}

/// Walk up from the tty's device dir to the USB device with idVendor.
fn usb_ids(dev: &Path) -> (u16, u16, String) {
    let mut d = std::fs::canonicalize(dev).unwrap_or_else(|_| dev.to_path_buf());
    for _ in 0..5 {
        let v = d.join("idVendor");
        if v.exists() {
            let h = |n: &str| u16::from_str_radix(&read(&d.join(n)), 16).unwrap_or(0);
            return (h("idVendor"), h("idProduct"), read(&d.join("product")));
        }
        if !d.pop() {
            break;
        }
    }
    (0, 0, String::new())
}

/// Serial ports under `tty_root` (normally /sys/class/tty), best first.
pub fn find_ports(tty_root: &Path, dev_dir: &Path) -> Vec<Port> {
    let Ok(rd) = std::fs::read_dir(tty_root) else { return vec![] };
    let mut v: Vec<Port> = rd
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if !(name.starts_with("ttyUSB") || name.starts_with("ttyACM")) {
                return None;
            }
            let devp = e.path().join("device");
            let driver = std::fs::read_link(devp.join("driver")).ok().and_then(|p| p.file_name().map(|s| s.to_string_lossy().into_owned())).unwrap_or_default();
            let (vid, pid, product) = usb_ids(&devp);
            Some(Port { dev: dev_dir.join(&name), name, driver, vid, pid, product })
        })
        .collect();
    v.sort_by_key(|p| (!p.is_pl2303(), !p.name.starts_with("ttyUSB"), p.name.clone()));
    v
}

/// Kernel has USB serial + PL2303 (built in or as a module)?
pub fn drivers_present() -> bool {
    if Path::new("/sys/bus/usb-serial/drivers/pl2303").exists() {
        return true;
    }
    let mut u: libc::utsname = unsafe { std::mem::zeroed() };
    if unsafe { libc::uname(&mut u) } != 0 {
        return true; // cannot tell: do not claim unsupported
    }
    let rel = unsafe { std::ffi::CStr::from_ptr(u.release.as_ptr()) }.to_string_lossy().into_owned();
    let base = PathBuf::from("/lib/modules").join(rel);
    if !base.exists() {
        return true;
    }
    let m = base.join("kernel/drivers/usb/serial");
    ["pl2303.ko", "pl2303.ko.xz", "pl2303.ko.gz", "pl2303.ko.zst"].iter().any(|f| m.join(f).exists())
        || std::fs::read_to_string(base.join("modules.builtin")).is_ok_and(|s| s.contains("usb/serial/pl2303.ko"))
}

/// Open read-only, raw, 4800 8N1, no flow control, non-blocking reads with
/// VMIN=0 / VTIME=2 (200 ms).
pub fn open_4800(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    let f = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NOCTTY | libc::O_CLOEXEC).open(path)?;
    let fd = f.as_raw_fd();
    unsafe {
        let mut t: libc::termios = std::mem::zeroed();
        if libc::tcgetattr(fd, &mut t) != 0 {
            return Err(std::io::Error::last_os_error());
        }
        libc::cfmakeraw(&mut t);
        t.c_cflag &= !(libc::CSTOPB | libc::PARENB | libc::CRTSCTS | libc::CSIZE);
        t.c_cflag |= libc::CS8 | libc::CLOCAL | libc::CREAD;
        t.c_iflag &= !(libc::IXON | libc::IXOFF | libc::IXANY);
        t.c_cc[libc::VMIN] = 0;
        t.c_cc[libc::VTIME] = 2;
        libc::cfsetispeed(&mut t, libc::B4800);
        libc::cfsetospeed(&mut t, libc::B4800);
        if libc::tcsetattr(fd, libc::TCSANOW, &t) != 0 {
            return Err(std::io::Error::last_os_error());
        }
        libc::tcflush(fd, libc::TCIFLUSH);
    }
    Ok(f)
}

#[derive(Debug, Clone, PartialEq)]
pub enum Msg {
    /// Scan found nothing (sent once per change).
    NoDevice,
    Opened(Port),
    /// Port exists but cannot be opened (permissions, busy).
    OpenError(Port, String),
    /// One NMEA line (CR/LF stripped).
    Line(String),
    /// Read failed / port vanished.
    Lost(String),
}

/// Splits a byte stream into lines; drops overlong garbage (wrong baud).
#[derive(Default)]
pub struct LineBuf {
    buf: Vec<u8>,
}

impl LineBuf {
    pub fn push(&mut self, data: &[u8], out: &mut Vec<String>) {
        for &b in data {
            match b {
                b'\n' | b'\r' => {
                    if !self.buf.is_empty() {
                        out.push(String::from_utf8_lossy(&self.buf).into_owned());
                        self.buf.clear();
                    }
                }
                _ => {
                    if self.buf.len() < 120 {
                        self.buf.push(b);
                    } else {
                        // no line end in 120 bytes: emit as one bad line so it is counted
                        out.push(String::from_utf8_lossy(&self.buf).into_owned());
                        self.buf.clear();
                    }
                }
            }
        }
    }
}

pub fn run_reader(tx: Sender<Msg>) {
    let mut last_none = false;
    loop {
        let ports = find_ports(Path::new("/sys/class/tty"), Path::new("/dev"));
        let Some(port) = ports.into_iter().next() else {
            if !last_none {
                last_none = true;
                if tx.send(Msg::NoDevice).is_err() {
                    return;
                }
            }
            std::thread::sleep(Duration::from_secs(1));
            continue;
        };
        last_none = false;
        let mut f = match open_4800(&port.dev) {
            Ok(f) => f,
            Err(e) => {
                if tx.send(Msg::OpenError(port, e.to_string())).is_err() {
                    return;
                }
                std::thread::sleep(Duration::from_secs(2));
                continue;
            }
        };
        eprintln!("gps: reading {} at 4800 8N1", port.describe());
        if tx.send(Msg::Opened(port.clone())).is_err() {
            return;
        }
        let mut lb = LineBuf::default();
        let mut lines = Vec::new();
        let mut buf = [0u8; 256];
        let mut idle = Instant::now();
        loop {
            match f.read(&mut buf) {
                Ok(0) => {
                    // VTIME timeout; a vanished USB tty reads 0 forever, so check the node
                    if !port.dev.exists() || idle.elapsed() > Duration::from_secs(2) && !Path::new("/sys/class/tty").join(&port.name).exists() {
                        let _ = tx.send(Msg::Lost("device unplugged".into()));
                        break;
                    }
                }
                Ok(n) => {
                    idle = Instant::now();
                    lb.push(&buf[..n], &mut lines);
                    for l in lines.drain(..) {
                        if tx.send(Msg::Line(l)).is_err() {
                            return;
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => {
                    let _ = tx.send(Msg::Lost(format!("read: {e}")));
                    break;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// `ZPOD_GPS_SIM=<file>`: replay an NMEA fixture as a fake device.
/// Lines: NMEA sentences; `#t=<ms>` = deliver the following lines at that
/// time; `#unplug` / `#plug`; `#ioerror <text>`; `#end` stops (port stays
/// open, no more data: lets the screen go stale).
pub fn run_sim(path: &str, tx: Sender<Msg>) {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let port = Port { dev: "/dev/ttyUSB0".into(), name: "ttyUSB0".into(), driver: "pl2303".into(), vid: 0x067b, pid: 0x2303, product: "USB-Serial Controller".into() };
    let t0 = Instant::now();
    let mut opened = false;
    let open = |tx: &Sender<Msg>, opened: &mut bool| {
        if !*opened {
            *opened = true;
            let _ = tx.send(Msg::Opened(port.clone()));
        }
    };
    for l in text.lines() {
        if let Some(ms) = l.strip_prefix("#t=") {
            let at = Duration::from_millis(ms.trim().parse().unwrap_or(0));
            std::thread::sleep(at.saturating_sub(t0.elapsed()));
            continue;
        }
        match l.split_whitespace().next() {
            Some("#unplug") => {
                opened = false;
                let _ = tx.send(Msg::Lost("device unplugged".into()));
                let _ = tx.send(Msg::NoDevice);
            }
            Some("#plug") => open(&tx, &mut opened),
            Some("#ioerror") => {
                let _ = tx.send(Msg::Lost(l["#ioerror".len()..].trim().to_string()));
            }
            Some("#end") => break,
            Some(c) if c.starts_with('#') => {}
            Some(_) => {
                open(&tx, &mut opened);
                if tx.send(Msg::Line(l.to_string())).is_err() {
                    return;
                }
            }
            None => {}
        }
    }
    // keep the channel alive (port "open", silent)
    loop {
        std::thread::sleep(Duration::from_secs(3600));
        if tx.send(Msg::Line(String::new())).is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_buffer_splits_and_caps() {
        let mut lb = LineBuf::default();
        let mut out = vec![];
        lb.push(b"$GPGGA,1*00\r\n$GPR", &mut out);
        assert_eq!(out, vec!["$GPGGA,1*00"]);
        lb.push(b"MC,2*00\r\n\r\n", &mut out);
        assert_eq!(out[1], "$GPRMC,2*00");
        assert_eq!(out.len(), 2);
        let junk = vec![b'x'; 300];
        out.clear();
        lb.push(&junk, &mut out);
        assert_eq!(out.len(), 2, "overlong garbage becomes bad lines, not memory");
    }

    #[test]
    fn sysfs_ports_prefer_pl2303() {
        let root = std::env::temp_dir().join(format!("zpod-gps-tty-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mk = |name: &str, vid: &str, pid: &str, drv: &str| {
            let usbdev = root.join("devices").join(name).join("usbdev");
            let intf = usbdev.join("1-1:1.0");
            let ttyd = intf.join(name);
            std::fs::create_dir_all(&ttyd).unwrap();
            std::fs::write(usbdev.join("idVendor"), vid).unwrap();
            std::fs::write(usbdev.join("idProduct"), pid).unwrap();
            std::fs::write(usbdev.join("product"), "USB-Serial Controller").unwrap();
            let drvdir = root.join("drivers").join(drv);
            std::fs::create_dir_all(&drvdir).unwrap();
            std::os::unix::fs::symlink(&drvdir, ttyd.join("driver")).unwrap();
            let cls = root.join("class").join(name);
            std::fs::create_dir_all(&cls).unwrap();
            std::os::unix::fs::symlink(&ttyd, cls.join("device")).unwrap();
        };
        mk("ttyACM0", "1546", "01a7", "cdc_acm");
        mk("ttyUSB1", "0403", "6001", "ftdi_sio");
        mk("ttyUSB2", "067b", "2303", "pl2303");
        std::fs::create_dir_all(root.join("class/ttyS0")).unwrap();
        let v = find_ports(&root.join("class"), Path::new("/dev"));
        let names: Vec<_> = v.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["ttyUSB2", "ttyUSB1", "ttyACM0"]);
        assert!(v[0].is_pl2303());
        assert_eq!((v[0].vid, v[0].pid), (0x067b, 0x2303));
        assert_eq!(v[0].describe(), "ttyUSB2 067b:2303 USB-Serial Controller");
        let _ = std::fs::remove_dir_all(&root);
    }
}
