//! Names for what is plugged in. The built-in table is deliberately small
//! (vendors and the devices ZPOD has tools for); users add lines to
//! `/data/zpod/appdata/usb-lab/known.txt`:
//!
//! ```text
//! # vvvv:pppp label      (product)      vvvv:* label   (any product of a vendor)
//! 148f:5370 Ralink RT5370 Wi-Fi (monitor ok)
//! 1234:* Acme Corp
//! ```

pub const USER_FILE: &str = "/data/zpod/appdata/usb-lab/known.txt";

/// (vid, pid or None = any, label, note)
pub type Entry = (u16, Option<u16>, String, &'static str);

const BUILTIN: &[(u16, Option<u16>, &str, &str)] = &[
    // devices ZPOD has a tool or app for
    (0x067b, Some(0x2303), "Prolific PL2303 serial (e.g. ND-105C GPS)", "GPS app"),
    (0x148f, Some(0x5370), "Ralink RT5370 Wi-Fi", "monitor-capable: Wi-Fi Lab"),
    (0x148f, Some(0x5372), "Ralink RT5372 Wi-Fi", "monitor-capable: Wi-Fi Lab"),
    (0x148f, Some(0x3070), "Ralink RT3070 Wi-Fi", "monitor-capable: Wi-Fi Lab"),
    (0x148f, Some(0x2870), "Ralink RT2870 Wi-Fi", "monitor-capable: Wi-Fi Lab"),
    (0x148f, Some(0x7601), "MediaTek MT7601U Wi-Fi", "monitor-capable: Wi-Fi Lab"),
    (0x0e8d, Some(0x7601), "MediaTek MT7601U Wi-Fi", "monitor-capable: Wi-Fi Lab"),
    (0x1546, Some(0x01a7), "u-blox 7 GPS", "GPS (cdc_acm)"),
    (0x1546, Some(0x01a8), "u-blox 8 GPS", "GPS (cdc_acm)"),
    (0x1d6b, Some(0x0104), "Linux composite gadget (e.g. another ZPOD)", ""),
    (0x1d6b, Some(0x0002), "Linux root hub 2.0", ""),
    (0x1915, Some(0x1025), "Nordic nRF52 USB device", ""),
    // vendors
    (0x05ac, None, "Apple", ""),
    (0x046d, None, "Logitech", ""),
    (0x045e, None, "Microsoft", ""),
    (0x04f2, None, "Chicony", ""),
    (0x413c, None, "Dell", ""),
    (0x17ef, None, "Lenovo", ""),
    (0x05e3, None, "Genesys Logic (hub/card reader)", ""),
    (0x0781, None, "SanDisk", ""),
    (0x0951, None, "Kingston", ""),
    (0x090c, None, "Silicon Motion (flash)", ""),
    (0x0bda, None, "Realtek", ""),
    (0x0403, None, "FTDI (serial)", ""),
    (0x10c4, None, "Silicon Labs (CP210x serial)", ""),
    (0x1a86, None, "QinHeng (CH340 serial)", ""),
    (0x067b, None, "Prolific", ""),
    (0x2341, None, "Arduino", ""),
    (0x2e8a, None, "Raspberry Pi (RP2040)", ""),
    (0x239a, None, "Adafruit", ""),
    (0x16c0, None, "Van Ooijen / PJRC (Teensy)", ""),
    (0x1209, None, "pid.codes (open hardware)", ""),
    (0x0483, None, "STMicroelectronics", ""),
    (0x04d8, None, "Microchip", ""),
    (0x1915, None, "Nordic Semiconductor", ""),
    (0x148f, None, "Ralink / MediaTek", ""),
    (0x0e8d, None, "MediaTek", ""),
    (0x0cf3, None, "Qualcomm Atheros", ""),
    (0x2357, None, "TP-Link", ""),
    (0x0846, None, "NETGEAR", ""),
    (0x1546, None, "u-blox", ""),
    (0x091e, None, "Garmin", ""),
    (0x0499, None, "Yamaha", ""),
    (0x0582, None, "Roland", ""),
    (0x0944, None, "Korg", ""),
    (0x1235, None, "Focusrite / Novation", ""),
    (0x1c75, None, "Arturia", ""),
    (0x2b73, None, "Pioneer DJ", ""),
    (0x1d6b, None, "Linux Foundation", ""),
    (0x1050, None, "Yubico", ""),
    (0x0a12, None, "Cambridge Silicon Radio (Bluetooth)", ""),
    (0x0b05, None, "ASUS", ""),
];

pub fn class_label(c: u8) -> &'static str {
    match c {
        0x00 => "per-interface",
        0x01 => "Audio",
        0x02 => "Communications (CDC)",
        0x03 => "HID",
        0x05 => "Physical",
        0x06 => "Image",
        0x07 => "Printer",
        0x08 => "Mass storage",
        0x09 => "Hub",
        0x0a => "CDC data",
        0x0b => "Smart card",
        0x0d => "Content security",
        0x0e => "Video",
        0x0f => "Healthcare",
        0x10 => "Audio/video",
        0x11 => "Billboard",
        0xdc => "Diagnostic",
        0xe0 => "Wireless (Bluetooth/RNDIS)",
        0xef => "Miscellaneous",
        0xfe => "Application specific (DFU)",
        0xff => "Vendor specific",
        _ => "?",
    }
}

/// Finer interface label from class/subclass/protocol.
pub fn iface_label(c: u8, s: u8, p: u8) -> String {
    match (c, s, p) {
        (1, 1, _) => "Audio control".into(),
        (1, 2, _) => "Audio streaming".into(),
        (1, 3, _) => "MIDI streaming".into(),
        (2, 2, _) => "CDC-ACM serial".into(),
        (2, 6, _) => "CDC-ECM Ethernet".into(),
        (2, 0x0d, _) => "CDC-NCM Ethernet".into(),
        (3, 1, 1) => "HID boot keyboard".into(),
        (3, 1, 2) => "HID boot mouse".into(),
        (3, _, _) => "HID".into(),
        (8, 6, 0x50) => "Mass storage (SCSI, bulk-only)".into(),
        (8, 6, 0x62) => "Mass storage (UAS)".into(),
        (0xe0, 1, 1) => "Bluetooth HCI".into(),
        (0xe0, 1, 3) => "RNDIS Ethernet".into(),
        (0xef, 4, 1) => "RNDIS Ethernet".into(),
        (0xfe, 1, _) => "DFU (firmware update)".into(),
        (c, s, p) => format!("{} ({:02x}/{:02x}/{:02x})", class_label(c), c, s, p),
    }
}

/// Parse user lines; bad lines are skipped.
pub fn parse_user(text: &str) -> Vec<Entry> {
    let mut v = Vec::new();
    for line in text.lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        let Some((id, label)) = l.split_once(char::is_whitespace) else { continue };
        let Some((vs, ps)) = id.split_once(':') else { continue };
        let Ok(vid) = u16::from_str_radix(vs, 16) else { continue };
        let pid = if ps == "*" {
            None
        } else {
            match u16::from_str_radix(ps, 16) {
                Ok(p) => Some(p),
                Err(_) => continue,
            }
        };
        let label: String = label.trim().chars().filter(|c| !c.is_control()).take(60).collect();
        if !label.is_empty() {
            v.push((vid, pid, label, "your list"));
        }
    }
    v
}

pub struct Known {
    pub user: Vec<Entry>,
}

impl Known {
    pub fn load() -> Known {
        Known { user: std::fs::read_to_string(USER_FILE).map(|t| parse_user(&t)).unwrap_or_default() }
    }
    pub fn empty() -> Known {
        Known { user: Vec::new() }
    }
    /// (product label, vendor label, note); user lines win.
    pub fn lookup(&self, vid: u16, pid: u16) -> (Option<String>, Option<String>, &'static str) {
        let mut prod = None;
        let mut vend = None;
        let mut note = "";
        for (v, p, l, n) in self.user.iter() {
            if *v == vid && *p == Some(pid) && prod.is_none() {
                prod = Some(l.clone());
                note = n;
            }
            if *v == vid && p.is_none() && vend.is_none() {
                vend = Some(l.clone());
            }
        }
        for (v, p, l, n) in BUILTIN {
            if *v == vid && *p == Some(pid) && prod.is_none() {
                prod = Some(l.to_string());
                note = n;
            }
            if *v == vid && p.is_none() && vend.is_none() {
                vend = Some(l.to_string());
            }
        }
        (prod, vend, note)
    }
}
