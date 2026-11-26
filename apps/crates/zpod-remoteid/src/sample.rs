// Copyright (C) 2026 Paul Lambert
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! Fixed sample payloads for tests in this and other crates (receive-path
//! fixtures: byte layouts copied from the spec tables, not a transmitter).

use crate::msg::MSG_LEN;

pub const UAS_ID: &str = "1596F3ZPOD0000000001";

pub fn basic_id() -> [u8; MSG_LEN] {
    let mut m = [0u8; MSG_LEN];
    m[0] = 0x02; // type 0, protocol 2
    m[1] = 0x12; // serial, multirotor
    m[2..22].copy_from_slice(UAS_ID.as_bytes());
    m
}

/// Airborne, AGL height, 90 deg, 10 m/s, +2 m/s, 37.7749/-122.4194,
/// baro 120 m, geo 125 m, height 50 m, 1234.5 s after the hour.
pub fn location() -> [u8; MSG_LEN] {
    let mut m = [0u8; MSG_LEN];
    m[0] = 0x12;
    m[1] = 0x24;
    m[2] = 90;
    m[3] = 40;
    m[4] = 4;
    m[5..9].copy_from_slice(&377_749_000i32.to_le_bytes());
    m[9..13].copy_from_slice(&(-1_224_194_000i32).to_le_bytes());
    m[13..15].copy_from_slice(&2240u16.to_le_bytes());
    m[15..17].copy_from_slice(&2250u16.to_le_bytes());
    m[17..19].copy_from_slice(&2100u16.to_le_bytes());
    m[19] = 0x4B;
    m[21..23].copy_from_slice(&12345u16.to_le_bytes());
    m[23] = 0x01;
    m
}

pub fn operator_id() -> [u8; MSG_LEN] {
    let mut m = [0u8; MSG_LEN];
    m[0] = 0x52;
    m[2..16].copy_from_slice(b"FIN87astrdge12");
    m
}

/// Message pack (0xF) of basic id + location + operator id.
pub fn pack() -> Vec<u8> {
    let mut v = vec![0xF2, MSG_LEN as u8, 3];
    v.extend_from_slice(&basic_id());
    v.extend_from_slice(&location());
    v.extend_from_slice(&operator_id());
    v
}

/// BLE4 service data (UUID FFFA, app code, counter 7, basic id).
pub fn ble_service_data() -> Vec<u8> {
    let mut v = vec![0xFA, 0xFF, 0x0D, 7];
    v.extend_from_slice(&basic_id());
    v
}

/// Vendor IE body for a Wi-Fi beacon (OUI FA0BBC, type 0D, counter 9, pack).
pub fn beacon_ie() -> Vec<u8> {
    let mut v = vec![0xFA, 0x0B, 0xBC, 0x0D, 9];
    v.extend_from_slice(&pack());
    v
}
