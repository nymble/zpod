// Copyright (C) 2026 Paul Lambert
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
use crate::ad;
use crate::classify::{classify, Class};
use crate::hci::{parse_event, parse_monitor, MON_EVENT_PKT};
use crate::table::{jsonl, Sort, Table};

fn legacy_event(addr: [u8; 6], addr_type: u8, evt: u8, data: &[u8], rssi: i8) -> Vec<u8> {
    let mut p = vec![0x02, 1, evt, addr_type];
    p.extend_from_slice(&addr);
    p.push(data.len() as u8);
    p.extend_from_slice(data);
    p.push(rssi as u8);
    let mut e = vec![0x3E, p.len() as u8];
    e.extend_from_slice(&p);
    e
}

fn monitor(ev: &[u8]) -> Vec<u8> {
    let mut m = MON_EVENT_PKT.to_le_bytes().to_vec();
    m.extend_from_slice(&0u16.to_le_bytes());
    m.extend_from_slice(&(ev.len() as u16).to_le_bytes());
    m.extend_from_slice(ev);
    m
}

fn rid_adv() -> Vec<u8> {
    let sd = zpod_remoteid::sample::ble_service_data();
    let mut d = vec![0x02, 0x01, 0x06, (sd.len() + 1) as u8, 0x16];
    d.extend_from_slice(&sd);
    d
}

fn airpods_adv() -> Vec<u8> {
    // mfr 004C, TLV type 07 len 0x19: 01 0e 20 ... (AirPods Pro)
    let mut v = vec![0x07, 0x19, 0x01, 0x0E, 0x20];
    v.extend_from_slice(&[0u8; 22]);
    let mut d = vec![(v.len() + 3) as u8, 0xFF, 0x4C, 0x00];
    d.extend_from_slice(&v);
    d
}

#[test]
fn legacy_report_through_monitor() {
    let ev = legacy_event([1, 2, 3, 4, 5, 0xC6], 1, 0, &rid_adv(), -70);
    let r = parse_monitor(&monitor(&ev));
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].addr, "C6:05:04:03:02:01");
    assert_eq!(r[0].addr_kind(), "random static");
    assert_eq!(r[0].rssi, -70);
    assert!(r[0].connectable());
    // a non-event monitor opcode is ignored
    let mut m = monitor(&ev);
    m[0] = 2;
    assert!(parse_monitor(&m).is_empty());
}

#[test]
fn extended_report() {
    let data = airpods_adv();
    let mut p = vec![0x0D, 1];
    p.extend_from_slice(&0x0013u16.to_le_bytes()); // connectable, scannable, legacy
    p.push(0); // public
    p.extend_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0x11]);
    p.extend_from_slice(&[1, 0, 0xFF, (-12i8) as u8, (-48i8) as u8, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    p.push(data.len() as u8);
    p.extend_from_slice(&data);
    let mut e = vec![0x3E, p.len() as u8];
    e.extend_from_slice(&p);
    let r = parse_event(&e);
    assert_eq!(r.len(), 1);
    assert_eq!((r[0].rssi, r[0].tx_power, r[0].addr.as_str()), (-48, Some(-12), "11:EE:DD:CC:BB:AA"));
    assert_eq!(r[0].addr_kind(), "public");
    let c = classify(&ad::parse(&r[0].data));
    assert_eq!(c, Class::Apple { t: 7, label: "AirPods Pro (pairing)".into() });
}

#[test]
fn classify_kinds() {
    let c = classify(&ad::parse(&rid_adv()));
    assert!(matches!(c, Class::RemoteId(7, ref m) if m.len() == 1));
    // iBeacon
    let mut ib = vec![0x1A, 0xFF, 0x4C, 0x00, 0x02, 0x15];
    ib.extend_from_slice(&[0x11; 16]);
    ib.extend_from_slice(&[0x00, 0x01, 0x00, 0x02, 0xC5]);
    assert!(matches!(classify(&ad::parse(&ib)), Class::IBeacon { major: 1, minor: 2, power: -59, .. }));
    // Eddystone URL
    let ed = [0x03, 0x03, 0xAA, 0xFE, 0x06, 0x16, 0xAA, 0xFE, 0x10, 0x00, 0x03];
    assert_eq!(classify(&ad::parse(&ed)), Class::Eddystone("URL"));
    // Find My (offline finding, type 0x12)
    let fm = [0x06, 0xFF, 0x4C, 0x00, 0x12, 0x01, 0x00];
    assert!(classify(&ad::parse(&fm)).notable());
    // unknown company
    let u = [0x05, 0xFF, 0x34, 0x12, 0x01, 0x02];
    assert_eq!(classify(&ad::parse(&u)).short(), "Company 0x1234");
    assert_eq!(classify(&ad::parse(&[0x05, 0x09, b'Z', b'P', b'O', b'D'])), Class::Generic);
}

#[test]
fn hostile_bytes_never_panic() {
    let ev = legacy_event([1, 2, 3, 4, 5, 6], 0, 0, &airpods_adv(), -50);
    for n in 0..ev.len() {
        let _ = parse_event(&ev[..n]);
        let _ = ad::parse(&ev[..n]);
    }
    let mut x: u32 = 99;
    for _ in 0..3000 {
        let mut b = [0u8; 64];
        for c in b.iter_mut() {
            x = x.wrapping_mul(1_103_515_245).wrapping_add(12345);
            *c = (x >> 16) as u8;
        }
        b[0] = 0x3E;
        let _ = parse_event(&b);
        let _ = classify(&ad::parse(&b));
    }
}

#[test]
fn table_merges_scan_response_and_sorts() {
    let mut t = Table::new(64);
    let adv = parse_event(&legacy_event([1, 0, 0, 0, 0, 0], 0, 0, &airpods_adv(), -60));
    let rsp = parse_event(&legacy_event([1, 0, 0, 0, 0, 0], 0, 4, &[0x05, 0x09, b'P', b'o', b'd', b's'], -58));
    let rid = parse_event(&legacy_event([2, 0, 0, 0, 0, 0xC0], 1, 3, &rid_adv(), -90));
    t.feed(&adv[0], 100);
    t.feed(&rsp[0], 200);
    t.feed(&rid[0], 300);
    assert_eq!(t.rows.len(), 2);
    let r = &t.rows[0];
    assert_eq!(r.name.as_deref(), Some("Pods"));
    assert!(matches!(r.class, Class::Apple { t: 7, .. }), "scan rsp must not erase the class");
    assert_eq!((r.rssi_min, r.rssi_max, r.count), (-60, -58, 2));
    // drones first in signal sort even when weaker
    assert_eq!(t.sorted(Sort::Signal)[0], 1);
    assert!(t.rows[1].title().starts_with("Drone 1596F3ZPOD"));
    assert!(t.rows[1].lines(400).iter().any(|l| l.starts_with("ID 1596F3ZPOD")));
    t.expire(10_000, 1000);
    assert!(t.rows.is_empty());
    let j = jsonl(&adv[0], 5, "Apple");
    assert!(j.starts_with("{\"t_ms\":5,\"addr\":\"00:00:00:00:00:01\""));
}

#[test]
fn table_evicts_stalest_when_full() {
    let mut t = Table::new(2);
    for k in 0..3u8 {
        let r = parse_event(&legacy_event([k, 0, 0, 0, 0, 0], 0, 0, &[2, 1, 6], -40));
        t.feed(&r[0], k as u64 * 10);
    }
    assert_eq!(t.rows.len(), 2);
    assert!(t.rows.iter().all(|r| r.addr != "00:00:00:00:00:00"));
}

/// tests/data/ble-replay.hex: the BLE Lab simulator feed (ZPOD_BLE_REPLAY),
/// one HCI monitor packet per line. ZPOD_REGEN_SAMPLES=1 rewrites it.
fn replay_lines() -> Vec<Vec<u8>> {
    let mut ib = vec![0x02, 0x01, 0x06, 0x1A, 0xFF, 0x4C, 0x00, 0x02, 0x15];
    ib.extend_from_slice(&[0xE2, 0xC5, 0x6D, 0xB5, 0xDF, 0xFB, 0x48, 0xD2, 0xB0, 0x60, 0xD0, 0xF5, 0xA7, 0x10, 0x96, 0xE0]);
    ib.extend_from_slice(&[0x00, 0x07, 0x00, 0x2A, 0xC5]);
    let ed = vec![0x03, 0x03, 0xAA, 0xFE, 0x0E, 0x16, 0xAA, 0xFE, 0x10, 0xEB, 0x03, b'z', b'p', b'o', b'd', b'.', b'd', b'e', b'v'];
    let tile = vec![0x02, 0x01, 0x06, 0x03, 0x03, 0xED, 0xFE];
    let named = vec![0x02, 0x01, 0x06, 0x0A, 0x09, b'Z', b'P', b'O', b'D', b'-', b'T', b'e', b's', b't'];
    let fm = vec![0x1E, 0xFF, 0x4C, 0x00, 0x12, 0x19, 0x10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let ev = [
        legacy_event([0x21, 0x43, 0x65, 0x87, 0xA9, 0xC2], 1, 0, &rid_adv(), -71),
        legacy_event([0x11, 0x22, 0x33, 0x44, 0x55, 0x66], 1, 0, &airpods_adv(), -44),
        legacy_event([0x01, 0x02, 0x03, 0x0A, 0x0B, 0x0C], 0, 3, &ib, -63),
        legacy_event([0x10, 0x20, 0x30, 0x40, 0x50, 0xE0], 1, 3, &ed, -80),
        legacy_event([0x99, 0x88, 0x77, 0x66, 0x55, 0xD4], 1, 0, &tile, -77),
        legacy_event([0x05, 0x04, 0x03, 0x02, 0x01, 0x00], 0, 0, &named, -52),
        legacy_event([0x3C, 0x2B, 0x1A, 0x09, 0xF8, 0x67], 1, 3, &fm, -85),
    ];
    ev.iter().map(|e| monitor(e)).collect()
}

#[test]
fn replay_sample_file() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/ble-replay.hex");
    let lines = replay_lines();
    let text: String = lines.iter().map(|l| l.iter().map(|b| format!("{b:02x}")).collect::<String>() + "\n").collect();
    if std::env::var_os("ZPOD_REGEN_SAMPLES").is_some() {
        std::fs::write(&path, format!("# BLE Lab simulator feed (ZPOD_BLE_REPLAY): HCI monitor packets, hex; from zpod-ble tests\n{text}")).unwrap();
    }
    let file = std::fs::read_to_string(&path).expect("tests/data/ble-replay.hex (ZPOD_REGEN_SAMPLES=1 cargo test -p zpod-ble)");
    let body: String = file.lines().filter(|l| !l.starts_with('#')).map(|l| format!("{l}\n")).collect();
    assert_eq!(body, text, "sample file is stale: ZPOD_REGEN_SAMPLES=1 cargo test -p zpod-ble");
    let mut t = Table::new(64);
    for (i, l) in lines.iter().enumerate() {
        for r in parse_monitor(l) {
            t.feed(&r, i as u64 * 100);
        }
    }
    let shorts: Vec<String> = t.rows.iter().map(|r| r.class.short()).collect();
    assert_eq!(t.rows.len(), 7, "{shorts:?}");
    for want in ["Remote ID", "Apple AirPods Pro (pairing)", "iBeacon", "Eddystone URL", "Tile tracker", "Apple Find My"] {
        assert!(shorts.iter().any(|s| s == want), "{want} not in {shorts:?}");
    }
}
