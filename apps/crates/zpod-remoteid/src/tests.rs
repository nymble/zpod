// Copyright (C) 2026 Paul Lambert
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
use crate::msg::*;
use crate::sample;
use crate::transport::*;
use crate::track::Tracker;

#[test]
fn basic_id_decodes() {
    let Some(Message::BasicId(b)) = decode(&sample::basic_id()) else { panic!() };
    assert_eq!(b.uas_id, sample::UAS_ID);
    assert_eq!(b.id_type, 1);
    assert_eq!(ua_type_label(b.ua_type), "multirotor");
}

#[test]
fn location_decodes() {
    let Some(Message::Location(l)) = decode(&sample::location()) else { panic!() };
    assert_eq!(status_label(l.status), "airborne");
    assert_eq!(l.track_deg, Some(90));
    assert_eq!(l.speed_h, Some(10.0));
    assert_eq!(l.speed_v, Some(2.0));
    assert!((l.lat.unwrap() - 37.7749).abs() < 1e-6);
    assert!((l.lon.unwrap() + 122.4194).abs() < 1e-6);
    assert_eq!(l.alt_baro, Some(120.0));
    assert_eq!(l.alt_geo, Some(125.0));
    assert_eq!(l.height, Some(50.0));
    assert!(l.height_agl);
    assert_eq!(l.tenths_after_hour, Some(12345));
    assert_eq!((l.v_acc, l.h_acc), (4, 11));
}

#[test]
fn location_unknowns() {
    let mut m = sample::location();
    m[3] = 255;
    m[4] = 126;
    m[5..13].fill(0);
    m[13..19].fill(0);
    m[21] = 0xff;
    m[22] = 0xff;
    let Some(Message::Location(l)) = decode(&m) else { panic!() };
    assert_eq!((l.speed_h, l.speed_v, l.lat, l.alt_baro, l.height, l.tenths_after_hour), (None, None, None, None, None, None));
}

#[test]
fn east_west_and_speed_multiplier() {
    let mut m = sample::location();
    m[1] = 0x23; // airborne, EW bit, multiplier
    m[2] = 10;
    m[3] = 10;
    let Some(Message::Location(l)) = decode(&m) else { panic!() };
    assert_eq!(l.track_deg, Some(190));
    assert_eq!(l.speed_h, Some(10.0 * 0.75 + 63.75));
}

#[test]
fn pack_decodes_all() {
    let v = decode_pack(&sample::pack());
    assert_eq!(v.len(), 3);
    assert_eq!(v[0].kind(), "Basic ID");
    assert_eq!(v[1].kind(), "Location");
    let Message::OperatorId(o) = &v[2] else { panic!() };
    assert_eq!(o.id, "FIN87astrdge12");
}

#[test]
fn truncated_and_garbage_never_panic() {
    let p = sample::pack();
    for n in 0..p.len() {
        let _ = decode_pack(&p[..n]);
        let _ = decode(&p[..n]);
    }
    let mut x: u32 = 0x1234_5678;
    for _ in 0..2000 {
        let mut b = [0u8; 80];
        for c in b.iter_mut() {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            *c = x as u8;
        }
        let _ = decode_pack(&b);
        let _ = from_ble_service_data(&b);
        let _ = from_wifi_beacon_ie(&b);
        let _ = from_nan_service_info(&b);
    }
    // a pack claiming 9 messages but carrying 1
    let mut short = vec![0xF2, 25, 9];
    short.extend_from_slice(&sample::basic_id());
    assert_eq!(decode_pack(&short).len(), 1);
    // wrong message size field
    assert!(decode_pack(&[0xF2, 24, 1, 0, 0, 0]).is_empty());
}

#[test]
fn ble_and_beacon_transports() {
    let (c, m) = from_ble_service_data(&sample::ble_service_data()).unwrap();
    assert_eq!((c, m.len()), (7, 1));
    assert!(from_ble_service_data(&[0xFB, 0xFF, 0x0D, 0]).is_none());
    let (c, m) = from_wifi_beacon_ie(&sample::beacon_ie()).unwrap();
    assert_eq!((c, m.len()), (9, 3));
    let mut bad = sample::beacon_ie();
    bad[0] = 0x00;
    assert!(from_wifi_beacon_ie(&bad).is_none());
}

#[test]
fn tracker_merges_and_expires() {
    let mut t = Tracker::new(10_000);
    let msgs = decode_pack(&sample::pack());
    t.feed("aa:bb:cc:dd:ee:ff", Transport::WifiBeacon, 1, &msgs, Some(-60), Some(6), 1000);
    t.feed("aa:bb:cc:dd:ee:ff", Transport::WifiBeacon, 2, &msgs[..1], Some(-55), None, 2000);
    assert_eq!(t.aircraft.len(), 1);
    let a = &t.aircraft[0];
    assert_eq!(a.label(), sample::UAS_ID);
    assert_eq!((a.frames, a.rssi, a.channel), (2, Some(-55), Some(6)));
    assert!(a.location.is_some());
    t.expire(20_000);
    assert!(t.aircraft.is_empty());
}

#[test]
fn display_lines() {
    let mut t = Tracker::new(0);
    t.feed("aa:bb:cc:dd:ee:ff", Transport::Ble4, 1, &decode_pack(&sample::pack()), Some(-70), None, 0);
    let l = crate::display::aircraft_lines(&t.aircraft[0], 1500);
    assert_eq!(l[0], format!("# {}", sample::UAS_ID));
    assert!(l.iter().any(|x| x.starts_with("airborne 37.774900, -122.419400")), "{l:?}");
    assert!(l.iter().any(|x| x == "Operator ID: FIN87astrdge12"));
}

#[test]
fn encode_roundtrips_sample_messages() {
    use crate::encode;
    for raw in [sample::basic_id(), sample::location(), sample::operator_id()] {
        let m = decode(&raw).unwrap();
        let again = match &m {
            Message::BasicId(b) => encode::basic_id(b),
            Message::Location(l) => encode::location(l),
            Message::OperatorId(o) => encode::operator_id(o),
            _ => unreachable!(),
        };
        assert_eq!(decode(&again).unwrap(), m);
    }
    // west-going track, fast (multiplier) speed, descending
    let mut l = match decode(&sample::location()).unwrap() { Message::Location(l) => l, _ => panic!() };
    l.track_deg = Some(271);
    l.speed_h = Some(80.5);
    l.speed_v = Some(-3.0);
    let Some(Message::Location(d)) = decode(&encode::location(&l)) else { panic!() };
    assert_eq!(d.track_deg, Some(271));
    assert!((d.speed_h.unwrap() - 80.5).abs() <= 0.4, "{:?}", d.speed_h);
    assert_eq!(d.speed_v, Some(-3.0));
}

#[test]
fn sim_moves_and_decodes_as_test_aircraft() {
    use crate::encode;
    use crate::sim::SimAircraft;
    let mut a = SimAircraft::test(37.7749, -122.4194);
    a.climb_mps = 1.0;
    let lon0 = a.lon;
    for _ in 0..10 {
        a.step(1.0);
    }
    // 100 m east at this latitude ~ 0.001137 deg lon; lat unchanged
    assert!((a.lon - lon0 - 0.001137).abs() < 0.00002, "{}", a.lon - lon0);
    assert!((a.lat - 37.7749).abs() < 1e-9);
    assert_eq!(a.alt_m, 60.0);
    a.turn_dps = 9.0;
    for _ in 0..10 {
        a.step(1.0);
    }
    assert!((a.track_deg - 180.0).abs() < 0.01);
    let ie = encode::beacon_ie(5, &encode::pack(&a.messages(1200, 1000)));
    let (c, msgs) = from_wifi_beacon_ie(&ie).unwrap();
    assert_eq!(c, 5);
    assert_eq!(msgs.len(), 5);
    let mut t = Tracker::new(0);
    t.feed("02:5a:50:00:7e:57", Transport::WifiBeacon, c, &msgs, None, Some(6), 0);
    let ac = &t.aircraft[0];
    assert_eq!(ac.label(), encode::TEST_UAS_ID);
    assert_eq!(ac.self_id.as_ref().unwrap().text, encode::TEST_SELF_ID);
    let loc = ac.location.as_ref().unwrap();
    assert_eq!(loc.track_deg, Some(180));
    assert_eq!(loc.speed_h, Some(10.0));
    assert!((loc.lat.unwrap() - a.lat).abs() < 2e-7);
}
