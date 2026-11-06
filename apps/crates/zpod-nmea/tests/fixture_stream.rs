//! Fixture stream through the state machine: no-fix -> 2D -> 3D -> stale,
//! one corrupted line tolerated (acceptance G2-G4, host side).

use zpod_nmea::{GpsState, GpsStatus};

#[test]
fn cold_start_fixture() {
    let text = include_str!("fixtures/nd105c-cold-start.nmea");
    let mut s = GpsState::default();
    let mut t = 0u64;
    let mut seen: Vec<(u64, GpsStatus)> = vec![];
    let mut bad = 0;
    let mut started = false;
    for l in text.lines() {
        if let Some(ms) = l.strip_prefix("#t=") {
            if started {
                seen.push((t, s.status(t + 900, true, None)));
            }
            t = ms.parse().unwrap();
            started = true;
            continue;
        }
        if l.starts_with('#') {
            continue;
        }
        if s.feed(l, t).is_err() {
            bad += 1;
        }
    }
    seen.push((t, s.status(t + 900, true, None)));
    let states: Vec<&GpsStatus> = seen.iter().map(|x| &x.1).collect();
    assert_eq!(states[0], &GpsStatus::NoFix, "{seen:?}");
    assert_eq!(states[4], &GpsStatus::NoFix, "sats in view, no fix");
    assert_eq!(states[5], &GpsStatus::Fix2D);
    assert_eq!(states[7], &GpsStatus::Fix3D);
    assert_eq!(states[9], &GpsStatus::Fix3D, "one bad checksum does not drop the fix");
    assert_eq!(bad, 1);
    assert_eq!(s.used_count(), Some(6));
    assert_eq!(s.sats_in_view(), 7);
    assert_eq!(s.sats().iter().filter(|x| x.snr.is_some()).count(), 6);
    assert!((s.lat.unwrap() - 37.774_935).abs() < 1e-5);
    assert!((s.lon.unwrap() + 122.419_413).abs() < 1e-5);
    assert_eq!(s.alt_m, Some(18.4));
    assert_eq!(s.date, Some((5, 11, 2026)));
    // no more data: stale after STALE_MS
    assert_eq!(s.status(t + zpod_nmea::STALE_MS + 1, true, None), GpsStatus::Stale);
    assert_eq!(s.status(t, false, None), GpsStatus::NoDevice);
}
