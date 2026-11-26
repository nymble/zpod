// Copyright (C) 2026 Paul Lambert
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
use std::fs;

use crate::frame::{self, Kind};
use crate::inventory::{freq_to_channel, inventory_at, is_onboard, parse_phy_info};
use crate::pcap;
use crate::radiotap;
use crate::rid::{decode_frame, Stats};
use zpod_remoteid::{sample, Tracker, Transport, NAN_SERVICE_ID};

/// radiotap: flags (FCS present) + channel 2437 + signal -42
fn rtap(fcs: bool) -> Vec<u8> {
    // present bits 1 (flags), 3 (channel), 5 (signal)
    let present: u32 = (1 << 1) | (1 << 3) | (1 << 5);
    let mut v = vec![0, 0, 0, 0];
    v.extend_from_slice(&present.to_le_bytes());
    v.push(if fcs { 0x10 } else { 0 }); // flags @8
    v.push(0); // pad to 2 for channel @10
    v.extend_from_slice(&2437u16.to_le_bytes());
    v.extend_from_slice(&0x00a0u16.to_le_bytes());
    v.push((-42i8) as u8); // signal @14
    let l = v.len() as u16;
    v[2..4].copy_from_slice(&l.to_le_bytes());
    v
}

const SA: [u8; 6] = [0x60, 0x60, 0x1f, 0x01, 0x02, 0x03];

fn mgmt(sub: u8) -> Vec<u8> {
    let mut f = vec![sub << 4, 0, 0, 0];
    f.extend_from_slice(&[0xff; 6]);
    f.extend_from_slice(&SA);
    f.extend_from_slice(&SA);
    f.extend_from_slice(&[0, 0]);
    f
}

fn beacon_with_rid() -> Vec<u8> {
    let mut f = mgmt(8);
    f.extend_from_slice(&[0u8; 12]); // timestamp, interval, capability
    f.extend_from_slice(&[0, 9]);
    f.extend_from_slice(b"DRONE-RID");
    let ie = sample::beacon_ie();
    f.push(221);
    f.push(ie.len() as u8);
    f.extend_from_slice(&ie);
    f
}

fn nan_sdf() -> Vec<u8> {
    let mut f = mgmt(13);
    f.extend_from_slice(&[0x04, 0x09, 0x50, 0x6F, 0x9A, 0x13]);
    let mut si = vec![3u8]; // counter
    si.extend_from_slice(&sample::pack());
    let mut sda = NAN_SERVICE_ID.to_vec();
    sda.extend_from_slice(&[1, 0, 0x10, si.len() as u8]);
    sda.extend_from_slice(&si);
    // an unrelated attribute first
    f.extend_from_slice(&[0x02, 2, 0, 0xAA, 0xBB]);
    f.push(0x03);
    f.extend_from_slice(&(sda.len() as u16).to_le_bytes());
    f.extend_from_slice(&sda);
    f
}

fn pkt(frame: &[u8], fcs: bool) -> Vec<u8> {
    let mut p = rtap(fcs);
    p.extend_from_slice(frame);
    if fcs {
        p.extend_from_slice(&[1, 2, 3, 4]);
    }
    p
}

#[test]
fn radiotap_fields() {
    let r = radiotap::parse(&rtap(true)).unwrap();
    assert_eq!((r.freq, r.signal, r.has_fcs(), r.len), (Some(2437), Some(-42), true, 15));
    assert!(radiotap::parse(&[1, 0, 8, 0, 0, 0, 0, 0]).is_none());
    assert!(radiotap::parse(&[0, 0, 200, 0, 0, 0, 0, 0]).is_none());
}

#[test]
fn beacon_remote_id() {
    let mut st = Stats::default();
    let d = decode_frame(&pkt(&beacon_with_rid(), true), &mut st).unwrap();
    assert_eq!(d.transport, Transport::WifiBeacon);
    assert_eq!(d.src, "60:60:1f:01:02:03");
    assert_eq!((d.counter, d.msgs.len(), d.rssi, d.freq), (9, 3, Some(-42), Some(2437)));
    assert_eq!(d.ssid.as_deref(), Some("DRONE-RID"));
    assert_eq!((st.beacons, st.rid), (1, 1));
}

#[test]
fn nan_remote_id() {
    let mut st = Stats::default();
    let d = decode_frame(&pkt(&nan_sdf(), false), &mut st).unwrap();
    assert_eq!(d.transport, Transport::WifiNan);
    assert_eq!((d.counter, d.msgs.len()), (3, 3));
    // a different service id is not Remote ID
    let mut other = nan_sdf();
    let p = other.windows(6).position(|w| w == NAN_SERVICE_ID).unwrap();
    other[p] ^= 0xff;
    assert!(decode_frame(&pkt(&other, false), &mut st).is_none());
}

#[test]
fn plain_beacon_and_hostile_frames() {
    let mut st = Stats::default();
    let mut f = mgmt(8);
    f.extend_from_slice(&[0u8; 12]);
    f.extend_from_slice(&[0, 4, b'h', b'o', b'm', b'e']);
    assert!(decode_frame(&pkt(&f, false), &mut st).is_none());
    assert_eq!(frame::ssid(&frame::beacon_ies(&f)).as_deref(), Some("home"));
    assert_eq!(frame::header(&[0x08, 0]).unwrap().kind, Kind::Data);
    let b = pkt(&beacon_with_rid(), true);
    for n in 0..b.len() {
        let _ = decode_frame(&b[..n], &mut st);
    }
    let n = pkt(&nan_sdf(), false);
    for k in 0..n.len() {
        let _ = decode_frame(&n[..k], &mut st);
    }
}

#[test]
fn pcap_roundtrip_and_tracker() {
    let mut w = pcap::Writer::new(Vec::new(), pcap::LINKTYPE_RADIOTAP).unwrap();
    w.packet(1_000_000, &pkt(&beacon_with_rid(), true)).unwrap();
    w.packet(1_500_000, &pkt(&nan_sdf(), false)).unwrap();
    let bytes = w.into_inner();
    let (lt, pk) = pcap::read_all(&bytes[..]).unwrap();
    assert_eq!((lt, pk.len()), (127, 2));
    let mut st = Stats::default();
    let mut t = Tracker::new(60_000);
    for (ts, p) in &pk {
        if let Some(d) = decode_frame(p, &mut st) {
            t.feed(&d.src, d.transport, d.counter, &d.msgs, d.rssi, d.freq.map(|f| freq_to_channel(f as u32)), ts / 1000);
        }
    }
    assert_eq!(t.aircraft.len(), 1);
    assert_eq!(t.aircraft[0].frames, 2);
    assert_eq!(t.aircraft[0].channel, Some(6));
    assert!(pcap::read_all(&bytes[..30]).is_err());
}

/// The sample capture in the repo (tests/data/rid-sample.pcap) decodes.
#[test]
fn repo_sample_pcap() {
    let p = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/rid-sample.pcap");
    let mut w = pcap::Writer::new(Vec::new(), pcap::LINKTYPE_RADIOTAP).unwrap();
    w.packet(1_000_000, &pkt(&beacon_with_rid(), true)).unwrap();
    w.packet(1_200_000, &pkt(&nan_sdf(), false)).unwrap();
    let fresh = w.into_inner();
    if std::env::var_os("ZPOD_REGEN_SAMPLES").is_some() {
        fs::create_dir_all(std::path::Path::new(p).parent().unwrap()).unwrap();
        fs::write(p, &fresh).unwrap();
    }
    let disk = fs::read(p).expect("tests/data/rid-sample.pcap (regenerate: ZPOD_REGEN_SAMPLES=1 cargo test -p zpod-wlan)");
    assert_eq!(disk, fresh, "sample pcap differs from the generator");
    let (_, pk) = pcap::read_all(&disk[..]).unwrap();
    let mut st = Stats::default();
    assert_eq!(pk.iter().filter_map(|(_, x)| decode_frame(x, &mut st)).count(), 2);
}

#[test]
fn phy_info_parse() {
    let t = "Wiphy phy1\n\tBand 1:\n\t\tFrequencies:\n\t\t\t* 2412.0 MHz [1] (20.0 dBm)\n\t\t\t* 2467.0 MHz [12] (disabled)\n\t\t\t* 2437 MHz [6] (20.0 dBm)\n\tSupported interface modes:\n\t\t * IBSS\n\t\t * managed\n\t\t * AP\n\t\t * monitor\n\tBand 2:\n";
    let c = parse_phy_info(t);
    assert!(c.monitor());
    assert_eq!(c.freqs, vec![2412, 2437]);
    assert!(!c.has_5ghz());
    let none = parse_phy_info("Wiphy phy0\n\tSupported interface modes:\n\t\t * managed\n\t\t * AP\n");
    assert!(!none.monitor());
}

#[test]
fn inventory_fake_sysfs() {
    let root = std::env::temp_dir().join(format!("zpod-wlan-test-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let net = root.join("net");
    let mk = |name: &str, drv: &str, bus: &str, ty: &str| {
        let d = net.join(name);
        fs::create_dir_all(&d).unwrap();
        let devd = root.join(format!("dev-{name}"));
        fs::create_dir_all(root.join("drivers").join(drv)).unwrap();
        fs::create_dir_all(root.join("bus").join(bus)).unwrap();
        fs::create_dir_all(&devd).unwrap();
        std::os::unix::fs::symlink(root.join("drivers").join(drv), devd.join("driver")).unwrap();
        std::os::unix::fs::symlink(root.join("bus").join(bus), devd.join("subsystem")).unwrap();
        std::os::unix::fs::symlink(&devd, d.join("device")).unwrap();
        fs::create_dir_all(root.join("ieee80211").join(format!("phy-{name}"))).unwrap();
        std::os::unix::fs::symlink(root.join("ieee80211").join(format!("phy-{name}")), d.join("phy80211")).unwrap();
        fs::write(d.join("type"), ty).unwrap();
        fs::write(d.join("operstate"), "up\n").unwrap();
        fs::write(d.join("address"), "02:00:00:00:00:01\n").unwrap();
    };
    mk("wlan0", "brcmfmac", "sdio", "1\n");
    mk("wlan1", "rt2800usb", "usb", "803\n");
    fs::create_dir_all(net.join("lo")).unwrap();
    let v = inventory_at(&net);
    fs::remove_dir_all(&root).ok();
    assert_eq!(v.len(), 2);
    assert!(v[0].onboard && !v[0].in_monitor());
    assert!(!v[1].onboard && v[1].in_monitor());
    assert_eq!(v[1].driver, "rt2800usb");
    assert!(is_onboard("brcmfmac", "usb"));
}

#[test]
fn testgen_track_pcap_decodes_as_moving_test_aircraft() {
    use crate::testgen;
    use zpod_remoteid::{encode, SimAircraft};
    let mut a = SimAircraft::test(37.7749, -122.4194);
    a.turn_dps = 3.0;
    let mut buf = Vec::new();
    assert_eq!(testgen::write_track_pcap(&mut buf, &mut a, 6, 30, 1.0, 1_763_600_000_000_000).unwrap(), 30);
    let (lt, pkts) = pcap::read_all(&buf[..]).unwrap();
    assert_eq!((lt, pkts.len()), (pcap::LINKTYPE_RADIOTAP, 30));
    let mut st = Stats::default();
    let mut t = Tracker::new(0);
    let mut lons = Vec::new();
    for (ts, p) in &pkts {
        let d = decode_frame(p, &mut st).expect("RID frame");
        assert_eq!(d.transport, Transport::WifiBeacon);
        assert_eq!(d.ssid.as_deref(), Some(testgen::TEST_SSID));
        assert_eq!(d.freq, Some(2437));
        t.feed(&d.src, d.transport, d.counter, &d.msgs, d.rssi, Some(6), ts / 1000);
        lons.push(t.aircraft[0].location.as_ref().unwrap().lon.unwrap());
    }
    assert_eq!((st.rid, t.aircraft.len()), (30, 1));
    assert!(lons.windows(2).all(|w| w[1] > w[0]), "moves east");
    let ac = &t.aircraft[0];
    assert_eq!(ac.label(), encode::TEST_UAS_ID);
    assert_eq!(ac.mac, "02:5a:50:00:7e:57");
    let trk = ac.location.as_ref().unwrap().track_deg.unwrap();
    assert!((175..=180).contains(&trk), "turned from 90 to ~177: {trk}");
}
