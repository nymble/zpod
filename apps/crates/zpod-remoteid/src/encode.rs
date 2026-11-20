//! OFFLINE encoder for clearly labelled TEST aircraft (ASTM F3411 layouts,
//! the exact inverse of [`crate::msg::decode`]).
//!
//! This module only builds bytes in memory, e.g. to write a test pcap on the
//! Mac. Nothing in ZPOD R1 can send them: `zpod-labd` has no transmit or
//! injection command, and no app gets a raw socket. Over-the-air test
//! transmission is a later, gated milestone (docs/ROADMAP-LABS.md: a
//! safeguard warning before every transmit, isolated lab only).

use crate::msg::{BasicId, Location, OperatorId, SelfId, System, MSG_LEN};

/// Every generated aircraft carries this marker in its Self ID.
pub const TEST_SELF_ID: &str = "ZPOD TEST - NOT A UA";
/// Serial-number style id used by the test generators.
pub const TEST_UAS_ID: &str = "ZPOD-TEST-0001";
/// Locally administered (0x02) transmitter address for test frames.
pub const TEST_MAC: [u8; 6] = [0x02, 0x5a, 0x50, 0x00, 0x7e, 0x57];

fn put_text(m: &mut [u8], s: &str) {
    for (d, c) in m.iter_mut().zip(s.bytes().filter(|b| b.is_ascii_graphic() || *b == b' ')) {
        *d = c;
    }
}

fn alt_raw(m: Option<f32>) -> u16 {
    match m {
        None => 0,
        Some(v) => (((v + 1000.0) * 2.0).round()).clamp(0.0, 65535.0) as u16,
    }
}

fn coord_raw(d: Option<f64>) -> i32 {
    d.map(|v| (v * 1e7).round().clamp(i32::MIN as f64, i32::MAX as f64) as i32).unwrap_or(0)
}

pub fn basic_id(b: &BasicId) -> [u8; MSG_LEN] {
    let mut m = [0u8; MSG_LEN];
    m[0] = 0x02;
    m[1] = (b.id_type << 4) | (b.ua_type & 0x0f);
    put_text(&mut m[2..22], &b.uas_id);
    m
}

pub fn location(l: &Location) -> [u8; MSG_LEN] {
    let mut m = [0u8; MSG_LEN];
    m[0] = 0x12;
    let (ew, dir) = match l.track_deg {
        None => (true, 181u16), // 361 = unknown
        Some(t) => {
            let t = t % 360;
            if t >= 180 { (true, t - 180) } else { (false, t) }
        }
    };
    let (mult, sp) = match l.speed_h {
        None => (false, 255u8),
        Some(s) if s <= 63.75 => (false, (s.max(0.0) / 0.25).round() as u8),
        Some(s) => (true, (((s - 63.75) / 0.75).round()).clamp(0.0, 254.0) as u8),
    };
    m[1] = (l.status << 4) | if l.height_agl { 0x04 } else { 0 } | if ew { 0x02 } else { 0 } | if mult { 0x01 } else { 0 };
    m[2] = dir as u8;
    m[3] = sp;
    m[4] = match l.speed_v {
        None => 126i8,
        Some(v) => (v / 0.5).round().clamp(-62.0, 62.0) as i8,
    } as u8;
    m[5..9].copy_from_slice(&coord_raw(l.lat).to_le_bytes());
    m[9..13].copy_from_slice(&coord_raw(l.lon).to_le_bytes());
    m[13..15].copy_from_slice(&alt_raw(l.alt_baro).to_le_bytes());
    m[15..17].copy_from_slice(&alt_raw(l.alt_geo).to_le_bytes());
    m[17..19].copy_from_slice(&alt_raw(l.height).to_le_bytes());
    m[19] = (l.v_acc << 4) | (l.h_acc & 0x0f);
    m[21..23].copy_from_slice(&l.tenths_after_hour.unwrap_or(0xffff).to_le_bytes());
    m
}

pub fn self_id(s: &SelfId) -> [u8; MSG_LEN] {
    let mut m = [0u8; MSG_LEN];
    m[0] = 0x32;
    m[1] = s.desc_type;
    put_text(&mut m[2..25], &s.text);
    m
}

pub fn system(s: &System) -> [u8; MSG_LEN] {
    let mut m = [0u8; MSG_LEN];
    m[0] = 0x42;
    m[1] = ((s.classification & 0x07) << 2) | (s.operator_loc_type & 0x03);
    m[2..6].copy_from_slice(&coord_raw(s.op_lat).to_le_bytes());
    m[6..10].copy_from_slice(&coord_raw(s.op_lon).to_le_bytes());
    m[10..12].copy_from_slice(&s.area_count.to_le_bytes());
    m[12] = (s.area_radius_m / 10).min(255) as u8;
    m[13..15].copy_from_slice(&alt_raw(s.area_ceiling).to_le_bytes());
    m[15..17].copy_from_slice(&alt_raw(s.area_floor).to_le_bytes());
    m[17] = (s.ua_category << 4) | (s.ua_class & 0x0f);
    m[18..20].copy_from_slice(&alt_raw(s.op_alt_geo).to_le_bytes());
    m[20..24].copy_from_slice(&s.timestamp.to_le_bytes());
    m
}

pub fn operator_id(o: &OperatorId) -> [u8; MSG_LEN] {
    let mut m = [0u8; MSG_LEN];
    m[0] = 0x52;
    m[1] = o.id_type;
    put_text(&mut m[2..22], &o.id);
    m
}

/// Message pack (type 0xF) of up to 9 messages.
pub fn pack(msgs: &[[u8; MSG_LEN]]) -> Vec<u8> {
    let n = msgs.len().min(9);
    let mut v = vec![0xF2, MSG_LEN as u8, n as u8];
    for m in &msgs[..n] {
        v.extend_from_slice(m);
    }
    v
}

/// Wi-Fi beacon vendor IE body (OUI FA:0B:BC, type 0x0D, counter, pack).
pub fn beacon_ie(counter: u8, pack: &[u8]) -> Vec<u8> {
    let mut v = vec![0xFA, 0x0B, 0xBC, 0x0D, counter];
    v.extend_from_slice(pack);
    v
}
