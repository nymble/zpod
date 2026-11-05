//! USB controller role of the Pi's data port, for honest UI states
//! ("USB is the Mac link right now" instead of "no device").

use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UsbRole {
    /// dwc2 dr_mode=peripheral: CDC-NCM gadget to the computer, no host.
    Gadget,
    /// dr_mode=host: keyboards, mice, GPS can be plugged in.
    Host,
    /// dr_mode=otg: decided by the cable (ID pin); treat as maybe-host.
    Otg,
    Unknown,
}

impl UsbRole {
    pub fn parse(s: &str) -> UsbRole {
        match s.trim_matches(|c: char| c == '\0' || c.is_whitespace()) {
            "peripheral" | "gadget" | "device" => UsbRole::Gadget,
            "host" => UsbRole::Host,
            "otg" => UsbRole::Otg,
            _ => UsbRole::Unknown,
        }
    }

    pub fn can_host(self) -> bool {
        matches!(self, UsbRole::Host | UsbRole::Otg)
    }

    pub fn label(self) -> &'static str {
        match self {
            UsbRole::Gadget => "gadget (computer link)",
            UsbRole::Host => "host",
            UsbRole::Otg => "OTG",
            UsbRole::Unknown => "unknown",
        }
    }

    /// One line for screens that need host mode.
    pub fn host_hint(self) -> &'static str {
        match self {
            UsbRole::Gadget => "USB is the computer link now (gadget mode)",
            UsbRole::Host => "USB host mode",
            UsbRole::Otg => "USB OTG: host with an OTG adapter",
            UsbRole::Unknown => "USB role unknown",
        }
    }
}

/// `ZPOD_USB_ROLE` (simulator, tests), else `/run/zpod/usb-role` (written by
/// S35usbgadget), else the device tree's dwc2 `dr_mode`.
pub fn usb_role() -> UsbRole {
    if let Ok(v) = std::env::var("ZPOD_USB_ROLE") {
        return UsbRole::parse(&v);
    }
    if let Ok(v) = std::fs::read_to_string("/run/zpod/usb-role") {
        let r = UsbRole::parse(&v);
        if r != UsbRole::Unknown {
            return r;
        }
    }
    dt_role(Path::new("/proc/device-tree/soc"))
}

/// dr_mode of the first `usb@*` node under `soc` (the Zero has one).
pub fn dt_role(soc: &Path) -> UsbRole {
    let Ok(rd) = std::fs::read_dir(soc) else { return UsbRole::Unknown };
    let mut nodes: Vec<_> = rd.flatten().filter(|e| e.file_name().to_string_lossy().starts_with("usb@")).collect();
    nodes.sort_by_key(|e| e.file_name());
    for n in nodes {
        if let Ok(b) = std::fs::read(n.path().join("dr_mode")) {
            return UsbRole::parse(&String::from_utf8_lossy(&b));
        }
    }
    UsbRole::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_roles() {
        assert_eq!(UsbRole::parse("peripheral\0"), UsbRole::Gadget);
        assert_eq!(UsbRole::parse("host\n"), UsbRole::Host);
        assert_eq!(UsbRole::parse("otg"), UsbRole::Otg);
        assert_eq!(UsbRole::parse(""), UsbRole::Unknown);
        assert!(UsbRole::Host.can_host() && !UsbRole::Gadget.can_host());
    }

    #[test]
    fn device_tree_fixture() {
        let root = std::env::temp_dir().join(format!("zpod-input-dt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("usb@7e980000")).unwrap();
        std::fs::write(root.join("usb@7e980000/dr_mode"), b"peripheral\0").unwrap();
        assert_eq!(dt_role(&root), UsbRole::Gadget);
        std::fs::write(root.join("usb@7e980000/dr_mode"), b"host\0").unwrap();
        assert_eq!(dt_role(&root), UsbRole::Host);
        assert_eq!(dt_role(&root.join("missing")), UsbRole::Unknown);
        let _ = std::fs::remove_dir_all(&root);
    }
}
