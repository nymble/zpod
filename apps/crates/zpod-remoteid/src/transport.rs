// Copyright (C) 2026 Paul Lambert
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! Locate Remote ID payloads inside received BLE / Wi-Fi structures.

use crate::msg::{decode_pack, Message};

/// 16-bit service UUID assigned to ASTM Remote ID.
pub const RID_BLE_UUID: u16 = 0xFFFA;
/// ASD-STAN OUI used in the Wi-Fi Beacon vendor IE.
pub const RID_OUI: [u8; 3] = [0xFA, 0x0B, 0xBC];
pub const RID_OUI_TYPE: u8 = 0x0D;
/// First 6 bytes of SHA-256("org.opendroneid.remoteid") (NAN service ID).
pub const NAN_SERVICE_ID: [u8; 6] = [0x88, 0x69, 0x19, 0x9D, 0x92, 0x09];
/// Application code following the UUID in BLE service data.
pub const BLE_APP_CODE: u8 = 0x0D;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transport {
    Ble4,
    Ble5,
    WifiBeacon,
    WifiNan,
}

impl Transport {
    pub fn label(self) -> &'static str {
        match self {
            Transport::Ble4 => "BLE4",
            Transport::Ble5 => "BLE5",
            Transport::WifiBeacon => "Wi-Fi beacon",
            Transport::WifiNan => "Wi-Fi NAN",
        }
    }
}

/// BLE AD type 0x16 payload (after the AD type byte): UUID16 LE, app code,
/// counter, message or pack. Returns (counter, messages).
pub fn from_ble_service_data(sd: &[u8]) -> Option<(u8, Vec<Message>)> {
    if sd.len() < 4 || u16::from_le_bytes([sd[0], sd[1]]) != RID_BLE_UUID || sd[2] != BLE_APP_CODE {
        return None;
    }
    let msgs = decode_pack(&sd[4..]);
    (!msgs.is_empty()).then_some((sd[3], msgs))
}

/// Wi-Fi vendor-specific IE body (after id 221 and length): OUI, type,
/// counter, message pack.
pub fn from_wifi_beacon_ie(ie: &[u8]) -> Option<(u8, Vec<Message>)> {
    if ie.len() < 5 || ie[0..3] != RID_OUI || ie[3] != RID_OUI_TYPE {
        return None;
    }
    let msgs = decode_pack(&ie[5..]);
    (!msgs.is_empty()).then_some((ie[4], msgs))
}

/// NAN service info bytes (from a Service Descriptor Attribute with
/// service ID [`NAN_SERVICE_ID`]): counter + message pack.
pub fn from_nan_service_info(si: &[u8]) -> Option<(u8, Vec<Message>)> {
    if si.len() < 2 {
        return None;
    }
    let msgs = decode_pack(&si[1..]);
    (!msgs.is_empty()).then_some((si[0], msgs))
}
