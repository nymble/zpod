// Copyright (C) 2026 Paul Lambert
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
use std::fs;
use std::path::Path;

use crate::desc;
use crate::dump;
use crate::hid;
use crate::known::{parse_user, Known};
use crate::scan::scan_at;

/// Boot keyboard report descriptor (HID 1.11 appendix B.1).
const KB_REPORT: &[u8] = &[
    0x05, 0x01, 0x09, 0x06, 0xA1, 0x01, 0x05, 0x07, 0x19, 0xE0, 0x29, 0xE7, 0x15, 0x00, 0x25, 0x01, 0x75, 0x01, 0x95, 0x08, 0x81, 0x02, 0x95, 0x01, 0x75, 0x08, 0x81, 0x01, 0x95, 0x05, 0x75, 0x01, 0x05,
    0x08, 0x19, 0x01, 0x29, 0x05, 0x91, 0x02, 0x95, 0x01, 0x75, 0x03, 0x91, 0x01, 0x95, 0x06, 0x75, 0x08, 0x15, 0x00, 0x25, 0x65, 0x05, 0x07, 0x19, 0x00, 0x29, 0x65, 0x81, 0x00, 0xC0,
];

/// Device descriptor + config with a HID keyboard and a mass storage
/// interface (the shape of a keystroke injector with storage).
fn combo_descriptors() -> Vec<u8> {
    let mut v = vec![18, 1, 0x00, 0x02, 0, 0, 0, 64, 0x34, 0x12, 0x78, 0x56, 0x00, 0x01, 1, 2, 3, 1];
    let mut cfg = vec![9, 2, 0, 0, 2, 1, 0, 0x80, 50];
    cfg.extend_from_slice(&[9, 4, 0, 0, 1, 3, 1, 1, 0]);
    cfg.extend_from_slice(&[9, 0x21, 0x11, 0x01, 0, 1, 0x22, KB_REPORT.len() as u8, 0]);
    cfg.extend_from_slice(&[7, 5, 0x81, 3, 8, 0, 10]);
    cfg.extend_from_slice(&[9, 4, 1, 0, 2, 8, 6, 0x50, 0]);
    cfg.extend_from_slice(&[7, 5, 0x82, 2, 0, 2, 0]);
    cfg.extend_from_slice(&[7, 5, 0x02, 2, 0, 2, 0]);
    let total = cfg.len() as u16;
    cfg[2..4].copy_from_slice(&total.to_le_bytes());
    v.extend_from_slice(&cfg);
    v
}

#[test]
fn descriptors_parse() {
    let p = desc::parse(&combo_descriptors());
    let d = p.device.unwrap();
    assert_eq!((d.vid, d.pid, d.bcd_usb), (0x1234, 0x5678, 0x0200));
    let c = &p.configs[0];
    assert_eq!(c.max_power_ma, 100);
    assert_eq!(c.interfaces.len(), 2);
    assert_eq!(c.interfaces[0].hid_report_len, Some(KB_REPORT.len() as u16));
    assert_eq!(c.interfaces[1].endpoints.len(), 2);
    assert_eq!(c.interfaces[1].endpoints[0].describe(), "EP 82 IN bulk 512B int 0");
    assert!(p.warnings.is_empty());
}

#[test]
fn descriptors_truncated_never_panic() {
    let b = combo_descriptors();
    for n in 0..b.len() {
        let _ = desc::parse(&b[..n]);
    }
    let p = desc::parse(&[0, 1, 2, 3]);
    assert_eq!(p.warnings.len(), 1);
}

#[test]
fn hid_keyboard() {
    let h = hid::parse(KB_REPORT);
    assert!(h.is_keyboard());
    assert_eq!(h.labels(), vec!["Keyboard".to_string()]);
    assert_eq!(h.reports.len(), 1);
    assert_eq!(h.reports[0].input_bits, 64);
    assert_eq!(h.reports[0].output_bits, 8);
    for n in 0..KB_REPORT.len() {
        let _ = hid::parse(&KB_REPORT[..n]);
    }
}

#[test]
fn hid_report_ids_and_vendor_page() {
    // mouse (id 1) + vendor collection (id 2) using a 4-byte usage
    let d = [0x05, 0x01, 0x09, 0x02, 0xA1, 0x01, 0x85, 0x01, 0x75, 0x08, 0x95, 0x03, 0x81, 0x02, 0xC0, 0x0B, 0x01, 0x00, 0x00, 0xFF, 0xA1, 0x01, 0x85, 0x02, 0x75, 0x08, 0x95, 0x40, 0xB1, 0x02, 0xC0];
    let h = hid::parse(&d);
    assert!(h.is_mouse());
    assert_eq!(h.labels()[1], "Vendor ff00:0001");
    assert_eq!(h.reports.iter().find(|r| r.id == 2).unwrap().feature_bits, 512);
}

#[test]
fn user_known_lines() {
    let u = parse_user("# c\n148f:5370 My RT5370\n1234:* Acme\nzz:12 bad\n0001:0002\n");
    assert_eq!(u.len(), 2);
    let k = Known { user: u };
    assert_eq!(k.lookup(0x148f, 0x5370).0.as_deref(), Some("My RT5370"));
    assert_eq!(k.lookup(0x1234, 1).1.as_deref(), Some("Acme"));
    assert_eq!(Known::empty().lookup(0x067b, 0x2303).2, "GPS app");
}

fn fake_sysfs(root: &Path) {
    let d = root.join("1-1");
    fs::create_dir_all(&d).unwrap();
    for (f, v) in [("idVendor", "1234"), ("idProduct", "5678"), ("bDeviceClass", "00"), ("busnum", "1"), ("devnum", "3"), ("speed", "12"), ("version", " 2.00"), ("manufacturer", "Acme")] {
        fs::write(d.join(f), format!("{v}\n")).unwrap();
    }
    fs::write(d.join("descriptors"), combo_descriptors()).unwrap();
    let i0 = root.join("1-1:1.0");
    fs::create_dir_all(i0.join("0003:1234:5678.0001")).unwrap();
    for (f, v) in [("bInterfaceClass", "03"), ("bInterfaceSubClass", "01"), ("bInterfaceProtocol", "01"), ("bInterfaceNumber", "00")] {
        fs::write(i0.join(f), format!("{v}\n")).unwrap();
    }
    fs::write(i0.join("0003:1234:5678.0001/report_descriptor"), KB_REPORT).unwrap();
    let i1 = root.join("1-1:1.1");
    fs::create_dir_all(&i1).unwrap();
    for (f, v) in [("bInterfaceClass", "08"), ("bInterfaceSubClass", "06"), ("bInterfaceProtocol", "50"), ("bInterfaceNumber", "01")] {
        fs::write(i1.join(f), format!("{v}\n")).unwrap();
    }
    fs::create_dir_all(root.join("usb1")).unwrap();
}

#[test]
fn scan_fake_sysfs_and_dump() {
    let root = std::env::temp_dir().join(format!("zpod-usb-test-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fake_sysfs(&root);
    let devs = scan_at(&root, &Known::empty());
    fs::remove_dir_all(&root).ok();
    assert_eq!(devs.len(), 1);
    let d = &devs[0];
    assert_eq!(d.id(), "1234:5678");
    assert_eq!(d.kinds(), "HID boot keyboard + Mass storage (SCSI, bulk-only)");
    let ind = d.indicators();
    assert!(ind.iter().any(|i| i.contains("Keyboard + storage")), "{ind:?}");
    assert!(ind.iter().any(|i| i.contains("unknown vendor")));
    let lines = dump::lines(d);
    assert!(lines.iter().any(|l| l.starts_with("HID 0: Keyboard")), "{lines:?}");
    let j = dump::json(&devs, "2026-11-20T00:00:00");
    assert!(j.contains("\"id\":\"1234:5678\""));
    assert!(j.contains("\"collections\":[\"Keyboard\"]"));
    let t = dump::text(&devs, "now");
    assert!(t.contains("descriptors: 1201"));
}

#[test]
fn json_escapes() {
    assert_eq!(dump::json_str("a\"b\\c\n\u{1}"), "\"a\\\"b\\\\c\\n\\u0001\"");
}
