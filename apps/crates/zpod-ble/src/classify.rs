// Copyright (C) 2026 Paul Lambert
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! What sent an advertisement. Receive-side classification only.
//!
//! Apple Continuity type numbers and the proximity-pairing model codes come
//! from public reverse-engineering (Celosia & Cunche 2020, "Discontinued
//! Privacy"; furiousMAC/continuity); they are labels, not guarantees.

use crate::ad::Ad;
use zpod_remoteid::{from_ble_service_data, Message};

#[derive(Clone, Debug, PartialEq)]
pub enum Class {
    /// ASTM F3411 Remote ID (counter, messages)
    RemoteId(u8, Vec<Message>),
    /// Apple Continuity message of type `t` with a label
    Apple { t: u8, label: String },
    IBeacon { uuid: String, major: u16, minor: u16, power: i8 },
    Eddystone(&'static str),
    GoogleFastPair,
    MicrosoftSwiftPair,
    Tile,
    Samsung,
    /// company id + name
    Company(u16, String),
    /// only a name / services
    Generic,
}

impl Class {
    pub fn short(&self) -> String {
        match self {
            Class::RemoteId(..) => "Remote ID".into(),
            Class::Apple { label, .. } => format!("Apple {label}"),
            Class::IBeacon { .. } => "iBeacon".into(),
            Class::Eddystone(k) => format!("Eddystone {k}"),
            Class::GoogleFastPair => "Fast Pair".into(),
            Class::MicrosoftSwiftPair => "Swift Pair".into(),
            Class::Tile => "Tile tracker".into(),
            Class::Samsung => "Samsung".into(),
            Class::Company(_, n) => n.clone(),
            Class::Generic => "BLE".into(),
        }
    }
    /// Highlight in lists (drones, trackers).
    pub fn notable(&self) -> bool {
        matches!(self, Class::RemoteId(..) | Class::Tile) || matches!(self, Class::Apple { t: 0x12, .. })
    }
}

pub fn company(id: u16) -> Option<&'static str> {
    Some(match id {
        0x0006 => "Microsoft",
        0x004C => "Apple",
        0x0075 => "Samsung",
        0x00E0 => "Google",
        0x0087 => "Garmin",
        0x02E5 => "Espressif",
        0x0059 => "Nordic Semiconductor",
        0x000F => "Broadcom",
        0x000D => "Texas Instruments",
        0x0499 => "Ruuvi",
        0x038F => "Xiaomi",
        0x0171 => "Amazon",
        0x0822 => "adafruit",
        0xFFFF => "test / unassigned",
        _ => return None,
    })
}

pub fn apple_type(t: u8) -> &'static str {
    match t {
        0x02 => "iBeacon",
        0x03 => "AirPrint",
        0x05 => "AirDrop",
        0x06 => "HomeKit",
        0x07 => "Proximity Pairing",
        0x08 => "Hey Siri",
        0x09 => "AirPlay Target",
        0x0A => "AirPlay Source",
        0x0B => "Magic Switch",
        0x0C => "Handoff",
        0x0D => "Tethering Target",
        0x0E => "Tethering Source",
        0x0F => "Nearby Action",
        0x10 => "Nearby Info",
        0x12 => "Find My",
        _ => "Continuity",
    }
}

/// Proximity Pairing model (2 bytes after the 0x01 prefix, big-endian).
pub fn apple_model(code: u16) -> Option<&'static str> {
    Some(match code {
        0x0220 => "AirPods",
        0x0F20 => "AirPods (2nd gen)",
        0x1320 => "AirPods (3rd gen)",
        0x1920 => "AirPods (4th gen)",
        0x0E20 => "AirPods Pro",
        0x1420 => "AirPods Pro (2nd gen)",
        0x2420 => "AirPods Pro (2nd gen, USB-C)",
        0x0A20 => "AirPods Max",
        0x0320 => "Powerbeats3",
        0x0B20 => "Powerbeats Pro",
        0x0520 => "BeatsX",
        0x0620 => "Beats Solo3",
        0x0920 => "Beats Studio3",
        0x1020 => "Beats Flex",
        0x1120 => "Beats Studio Buds",
        _ => return None,
    })
}

/// Apple TLVs: (type, data).
pub fn apple_tlvs(d: &[u8]) -> Vec<(u8, &[u8])> {
    let mut v = Vec::new();
    let mut i = 0;
    while i + 2 <= d.len() {
        let (t, l) = (d[i], d[i + 1] as usize);
        let Some(x) = d.get(i + 2..i + 2 + l) else { break };
        v.push((t, x));
        i += 2 + l;
    }
    v
}

fn uuid_str(b: &[u8]) -> String {
    let h: String = b.iter().map(|x| format!("{x:02X}")).collect();
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

pub fn classify(a: &Ad) -> Class {
    for (u, d) in &a.service_data {
        if *u == zpod_remoteid::RID_BLE_UUID {
            let mut sd = u.to_le_bytes().to_vec();
            sd.extend_from_slice(d);
            if let Some((c, m)) = from_ble_service_data(&sd) {
                return Class::RemoteId(c, m);
            }
        }
    }
    for (c, d) in &a.mfr {
        if *c == 0x004C {
            let tl = apple_tlvs(d);
            if let Some((0x02, x)) = tl.first().copied() {
                if x.len() >= 21 {
                    return Class::IBeacon { uuid: uuid_str(&x[0..16]), major: u16::from_be_bytes([x[16], x[17]]), minor: u16::from_be_bytes([x[18], x[19]]), power: x[20] as i8 };
                }
            }
            if let Some(&(t, x)) = tl.iter().find(|(t, _)| *t == 0x07).or(tl.first()) {
                let mut label = apple_type(t).to_string();
                if t == 0x07 && x.len() >= 3 {
                    if let Some(m) = apple_model(u16::from_be_bytes([x[1], x[2]])) {
                        label = format!("{m} (pairing)");
                    }
                }
                return Class::Apple { t, label };
            }
            return Class::Apple { t: 0, label: "device".into() };
        }
        if *c == 0x0006 && d.first() == Some(&0x03) {
            return Class::MicrosoftSwiftPair;
        }
    }
    for (u, d) in &a.service_data {
        match *u {
            0xFEAA => {
                let k = match d.first() {
                    Some(0x00) => "UID",
                    Some(0x10) => "URL",
                    Some(0x20) => "TLM",
                    Some(0x30) => "EID",
                    _ => "frame",
                };
                return Class::Eddystone(k);
            }
            0xFE2C => return Class::GoogleFastPair,
            0xFEED | 0xFEEC => return Class::Tile,
            _ => {}
        }
    }
    if a.uuid16.iter().any(|u| matches!(u, 0xFEED | 0xFEEC)) {
        return Class::Tile;
    }
    if let Some((c, _)) = a.mfr.first() {
        if *c == 0x0075 {
            return Class::Samsung;
        }
        return Class::Company(*c, company(*c).map(str::to_string).unwrap_or_else(|| format!("Company 0x{c:04X}")));
    }
    Class::Generic
}
