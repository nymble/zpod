//! Walk `/sys/bus/usb/devices` (root path injectable for tests).

use std::fs;
use std::path::{Path, PathBuf};

use crate::desc::{self, Parsed};
use crate::hid::{self, HidSummary};
use crate::known::{iface_label, Known};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Interface {
    /// "1-1:1.0"
    pub name: String,
    pub number: u8,
    pub class: u8,
    pub subclass: u8,
    pub protocol: u8,
    pub label: String,
    pub driver: Option<String>,
    pub hid: Option<HidSummary>,
    /// raw report descriptor (for dumps)
    pub hid_raw: Vec<u8>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Device {
    /// sysfs name, e.g. "1-1.2" (bus-port path)
    pub path: String,
    pub busnum: u32,
    pub devnum: u32,
    pub vid: u16,
    pub pid: u16,
    pub class: u8,
    pub manufacturer: Option<String>,
    pub product: Option<String>,
    pub serial: Option<String>,
    /// Mb/s ("1.5", "12", "480")
    pub speed: Option<String>,
    pub version: Option<String>,
    pub max_power: Option<String>,
    pub known_product: Option<String>,
    pub known_vendor: Option<String>,
    pub note: &'static str,
    pub interfaces: Vec<Interface>,
    pub parsed: Parsed,
    pub raw: Vec<u8>,
}

impl Device {
    pub fn id(&self) -> String {
        format!("{:04x}:{:04x}", self.vid, self.pid)
    }
    /// Short name: known product, else product string, else vendor + id.
    pub fn title(&self) -> String {
        if let Some(p) = &self.known_product {
            return p.clone();
        }
        match (&self.manufacturer, &self.product) {
            (_, Some(p)) if !p.is_empty() => p.clone(),
            _ => format!("{} {}", self.known_vendor.clone().unwrap_or_else(|| "Unknown vendor".into()), self.id()),
        }
    }
    pub fn is_hub(&self) -> bool {
        self.class == 9
    }
    /// One line of interface kinds: "HID boot keyboard + Mass storage".
    pub fn kinds(&self) -> String {
        let mut v: Vec<&str> = Vec::new();
        for i in &self.interfaces {
            if !v.contains(&i.label.as_str()) {
                v.push(&i.label);
            }
        }
        v.join(" + ")
    }
    /// BadUSB-style indicators: things a keystroke injector or
    /// multi-personality attack tool tends to show. Hints, not verdicts.
    pub fn indicators(&self) -> Vec<String> {
        let mut v = Vec::new();
        let kb = self.interfaces.iter().any(|i| i.class == 3 && (i.protocol == 1 || i.hid.as_ref().is_some_and(|h| h.is_keyboard())));
        let storage = self.interfaces.iter().any(|i| i.class == 8);
        let net = self.interfaces.iter().any(|i| (i.class == 2 && matches!(i.subclass, 6 | 0x0d)) || (i.class == 0xe0 && i.protocol == 3) || (i.class == 0xef && i.subclass == 4));
        let serial = self.interfaces.iter().any(|i| i.class == 2 && i.subclass == 2);
        if kb && storage {
            v.push("Keyboard + storage in one device".into());
        }
        if kb && net {
            v.push("Keyboard + network adapter in one device".into());
        }
        if kb && serial {
            v.push("Keyboard + serial port in one device".into());
        }
        if kb && self.known_vendor.is_none() && self.known_product.is_none() {
            v.push("Keyboard from an unknown vendor ID".into());
        }
        if kb && self.product.as_deref().is_none_or(str::is_empty) {
            v.push("Keyboard with no product name".into());
        }
        if self.vid == 0x1d6b && !matches!(self.pid, 1..=3) {
            v.push("Linux gadget IDs (a programmable device)".into());
        }
        v
    }
}

fn rd(p: &Path) -> Option<String> {
    fs::read_to_string(p).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}
fn hex8(p: &Path) -> u8 {
    rd(p).and_then(|s| u8::from_str_radix(&s, 16).ok()).unwrap_or(0)
}
fn hex16(p: &Path) -> u16 {
    rd(p).and_then(|s| u16::from_str_radix(&s, 16).ok()).unwrap_or(0)
}

fn read_hid(ifdir: &Path) -> Option<(HidSummary, Vec<u8>)> {
    // <iface>/<0003:VVVV:PPPP.NNNN>/report_descriptor
    for e in fs::read_dir(ifdir).ok()?.flatten() {
        let n = e.file_name().to_string_lossy().to_string();
        if n.len() > 10 && n.as_bytes()[4] == b':' {
            if let Ok(raw) = fs::read(e.path().join("report_descriptor")) {
                return Some((hid::parse(&raw), raw));
            }
        }
    }
    None
}

/// Every non-root-hub device under `root` (normally /sys/bus/usb/devices).
pub fn scan_at(root: &Path, known: &Known) -> Vec<Device> {
    let mut out = Vec::new();
    let Ok(rd_root) = fs::read_dir(root) else { return out };
    let mut names: Vec<String> = rd_root.flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
    names.sort();
    for n in names.iter() {
        if n.contains(':') || n.starts_with("usb") {
            continue;
        }
        let d: PathBuf = root.join(n);
        if !d.join("idVendor").exists() {
            continue;
        }
        let raw = fs::read(d.join("descriptors")).unwrap_or_default();
        let mut dev = Device {
            path: n.clone(),
            busnum: rd(&d.join("busnum")).and_then(|s| s.parse().ok()).unwrap_or(0),
            devnum: rd(&d.join("devnum")).and_then(|s| s.parse().ok()).unwrap_or(0),
            vid: hex16(&d.join("idVendor")),
            pid: hex16(&d.join("idProduct")),
            class: hex8(&d.join("bDeviceClass")),
            manufacturer: rd(&d.join("manufacturer")),
            product: rd(&d.join("product")),
            serial: rd(&d.join("serial")),
            speed: rd(&d.join("speed")),
            version: rd(&d.join("version")),
            max_power: rd(&d.join("bMaxPower")),
            parsed: desc::parse(&raw),
            raw,
            ..Default::default()
        };
        let (kp, kv, note) = known.lookup(dev.vid, dev.pid);
        (dev.known_product, dev.known_vendor, dev.note) = (kp, kv, note);
        let prefix = format!("{n}:");
        for m in names.iter().filter(|m| m.starts_with(&prefix)) {
            let ip = root.join(m);
            let (c, s, p) = (hex8(&ip.join("bInterfaceClass")), hex8(&ip.join("bInterfaceSubClass")), hex8(&ip.join("bInterfaceProtocol")));
            let driver = fs::read_link(ip.join("driver")).ok().and_then(|l| l.file_name().map(|f| f.to_string_lossy().to_string()));
            let (hid, hid_raw) = if c == 3 { read_hid(&ip).map(|(h, r)| (Some(h), r)).unwrap_or((None, Vec::new())) } else { (None, Vec::new()) };
            dev.interfaces.push(Interface {
                name: m.clone(),
                number: hex8(&ip.join("bInterfaceNumber")),
                class: c,
                subclass: s,
                protocol: p,
                label: iface_label(c, s, p),
                driver,
                hid,
                hid_raw,
            });
        }
        // no sysfs interfaces (e.g. unconfigured): fall back to descriptors
        if dev.interfaces.is_empty() {
            if let Some(cfg) = dev.parsed.configs.first() {
                for i in cfg.interfaces.iter().filter(|i| i.alt == 0) {
                    dev.interfaces.push(Interface { name: format!("{n}:{}.{}", cfg.value, i.number), number: i.number, class: i.class, subclass: i.subclass, protocol: i.protocol, label: iface_label(i.class, i.subclass, i.protocol), ..Default::default() });
                }
            }
        }
        out.push(dev);
    }
    out
}

pub fn scan(known: &Known) -> Vec<Device> {
    scan_at(Path::new("/sys/bus/usb/devices"), known)
}
