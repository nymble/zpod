// Copyright (C) 2026 Paul Lambert
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! Advertising data: a sequence of [len][type][payload] structures.

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ad {
    pub flags: Option<u8>,
    pub name: Option<String>,
    pub tx_power: Option<i8>,
    pub uuid16: Vec<u16>,
    pub uuid128: Vec<[u8; 16]>,
    /// (uuid16, data after the uuid)
    pub service_data: Vec<(u16, Vec<u8>)>,
    /// (company id, data after the id)
    pub mfr: Vec<(u16, Vec<u8>)>,
    pub appearance: Option<u16>,
    /// structures we do not decode: (type, len)
    pub other: Vec<(u8, usize)>,
    pub malformed: bool,
}

pub fn parse(b: &[u8]) -> Ad {
    let mut a = Ad::default();
    let mut i = 0usize;
    while i < b.len() {
        let len = b[i] as usize;
        if len == 0 {
            break; // padding
        }
        if i + 1 + len > b.len() {
            a.malformed = true;
            break;
        }
        let ty = b[i + 1];
        let d = &b[i + 2..i + 1 + len];
        match ty {
            0x01 if !d.is_empty() => a.flags = Some(d[0]),
            0x02 | 0x03 => a.uuid16.extend(d.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]]))),
            0x06 | 0x07 => a.uuid128.extend(d.chunks_exact(16).map(|c| {
                let mut u = [0u8; 16];
                u.copy_from_slice(c);
                u
            })),
            0x08 | 0x09 => {
                let n: String = String::from_utf8_lossy(d).chars().filter(|c| !c.is_control()).collect();
                if ty == 0x09 || a.name.is_none() {
                    a.name = Some(n);
                }
            }
            0x0A if !d.is_empty() => a.tx_power = Some(d[0] as i8),
            0x16 if d.len() >= 2 => a.service_data.push((u16::from_le_bytes([d[0], d[1]]), d[2..].to_vec())),
            0x19 if d.len() >= 2 => a.appearance = Some(u16::from_le_bytes([d[0], d[1]])),
            0xFF if d.len() >= 2 => a.mfr.push((u16::from_le_bytes([d[0], d[1]]), d[2..].to_vec())),
            t => a.other.push((t, d.len())),
        }
        i += 1 + len;
    }
    a
}
