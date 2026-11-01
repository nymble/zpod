//! Tools > USB (test3): a landing page for the USB gadget link. Reads
//! sysfs/configfs and the ARP table only; nothing is sent to the host.
//! Bash Bunny-style tools come later.

use std::path::Path;

use zpod_hal::{sys, wifi};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Info {
    /// /sys/class/udc/*/state: "configured", "not attached", ...
    pub udc_state: Option<String>,
    /// /sys/class/udc/*/current_speed: "high-speed", ...
    pub speed: Option<String>,
    /// "CDC-NCM" / "CDC-ECM" / "RNDIS" from the gadget's functions
    pub function: Option<String>,
    pub link_up: bool,
    pub ip: Option<String>,
    pub host_ip: Option<String>,
    pub host_mac: Option<String>,
    pub host_name: Option<String>,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

fn read(p: impl AsRef<Path>) -> Option<String> {
    std::fs::read_to_string(p).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

pub fn function_name(dir_entry: &str) -> Option<&'static str> {
    let kind = dir_entry.split('.').next()?;
    Some(match kind {
        "ncm" => "CDC-NCM",
        "ecm" => "CDC-ECM",
        "eem" => "CDC-EEM",
        "rndis" => "RNDIS",
        "acm" => "Serial (ACM)",
        "mass_storage" => "Mass storage",
        "hid" => "HID",
        _ => return None,
    })
}

pub fn gather() -> Info {
    if sys::sim() {
        let ip = sys::ipv4("usb0");
        let up = ip.is_some();
        return Info {
            udc_state: Some(if up { "configured" } else { "not attached" }.into()),
            speed: up.then(|| "high-speed".into()),
            function: Some("CDC-NCM".into()),
            link_up: up,
            ip,
            host_ip: up.then(|| "10.12.194.2".into()),
            host_mac: up.then(|| "02:5a:50:4f:44:01".into()),
            host_name: up.then(|| "Pauls-MacBook".into()),
            rx_bytes: if up { 1_482_113 } else { 0 },
            tx_bytes: if up { 9_210_554 } else { 0 },
        };
    }
    let mut i = Info::default();
    if let Ok(rd) = std::fs::read_dir("/sys/class/udc") {
        if let Some(e) = rd.flatten().next() {
            i.udc_state = read(e.path().join("state"));
            i.speed = read(e.path().join("current_speed")).filter(|s| s != "UNKNOWN");
        }
    }
    if let Ok(gs) = std::fs::read_dir("/sys/kernel/config/usb_gadget") {
        for g in gs.flatten() {
            if let Ok(fs) = std::fs::read_dir(g.path().join("functions")) {
                let names: Vec<&str> = fs.flatten().filter_map(|f| function_name(&f.file_name().to_string_lossy())).collect();
                if !names.is_empty() {
                    i.function = Some(names.join(" + "));
                }
            }
        }
    }
    let n = Path::new("/sys/class/net/usb0");
    i.link_up = read(n.join("carrier")).as_deref() == Some("1") && read(n.join("operstate")).as_deref() != Some("down");
    i.ip = sys::ipv4("usb0");
    i.rx_bytes = read(n.join("statistics/rx_bytes")).and_then(|s| s.parse().ok()).unwrap_or(0);
    i.tx_bytes = read(n.join("statistics/tx_bytes")).and_then(|s| s.parse().ok()).unwrap_or(0);
    let arp = wifi::parse_arp(&std::fs::read_to_string("/proc/net/arp").unwrap_or_default(), "usb0");
    if let Some((ip, mac)) = arp.first() {
        i.host_ip = Some(ip.clone());
        i.host_mac = Some(mac.clone());
    }
    // host name from the DHCP lease, if the host sent one
    let mut leases = String::new();
    for f in ["/var/lib/misc/dnsmasq.leases", "/var/lib/NetworkManager/dnsmasq-usb0.leases"] {
        leases.push_str(&std::fs::read_to_string(f).unwrap_or_default());
    }
    if Path::new("/run/udhcpd.usb0.leases").exists() {
        if let Ok(o) = std::process::Command::new("dumpleases").args(["-f", "/run/udhcpd.usb0.leases"]).output() {
            leases.push_str(&String::from_utf8_lossy(&o.stdout));
        }
    }
    for (mac, ip, host) in wifi::parse_leases(&leases) {
        if ip.starts_with("10.12.194.") && (i.host_mac.is_none() || i.host_mac.as_deref() == Some(mac.as_str())) {
            i.host_ip.get_or_insert(ip);
            i.host_mac.get_or_insert(mac);
            i.host_name = host;
        }
    }
    i
}

#[cfg(test)]
mod tests {
    #[test]
    fn functions() {
        assert_eq!(super::function_name("ncm.usb0"), Some("CDC-NCM"));
        assert_eq!(super::function_name("rndis.usb0"), Some("RNDIS"));
        assert_eq!(super::function_name("os_desc"), None);
    }
}
