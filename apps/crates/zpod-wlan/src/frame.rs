// Copyright (C) 2026 Paul Lambert
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! 802.11 MAC frames: just enough of management frames to find SSIDs,
//! vendor IEs and NAN service discovery frames.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Beacon,
    ProbeReq,
    ProbeResp,
    Action,
    OtherMgmt(u8),
    Control,
    Data,
    Ext,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Header {
    pub kind: Kind,
    pub da: [u8; 6],
    pub sa: [u8; 6],
    pub bssid: [u8; 6],
}

pub fn mac_str(m: &[u8; 6]) -> String {
    format!("{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}", m[0], m[1], m[2], m[3], m[4], m[5])
}

fn mac(b: &[u8]) -> [u8; 6] {
    let mut m = [0u8; 6];
    m.copy_from_slice(&b[..6]);
    m
}

/// Frame control + addresses of a management frame (24-byte header).
/// Control/data frames return their kind with zero addresses.
pub fn header(f: &[u8]) -> Option<Header> {
    if f.len() < 2 {
        return None;
    }
    let ty = (f[0] >> 2) & 3;
    let sub = f[0] >> 4;
    let kind = match (ty, sub) {
        (0, 8) => Kind::Beacon,
        (0, 4) => Kind::ProbeReq,
        (0, 5) => Kind::ProbeResp,
        (0, 13) => Kind::Action,
        (0, s) => Kind::OtherMgmt(s),
        (1, _) => Kind::Control,
        (2, _) => Kind::Data,
        _ => Kind::Ext,
    };
    if ty != 0 {
        return Some(Header { kind, da: [0; 6], sa: [0; 6], bssid: [0; 6] });
    }
    if f.len() < 24 {
        return None;
    }
    Some(Header { kind, da: mac(&f[4..]), sa: mac(&f[10..]), bssid: mac(&f[16..]) })
}

/// Information elements: (id, body).
pub fn ies(b: &[u8]) -> Vec<(u8, &[u8])> {
    let mut v = Vec::new();
    let mut i = 0usize;
    while i + 2 <= b.len() {
        let (id, l) = (b[i], b[i + 1] as usize);
        let Some(body) = b.get(i + 2..i + 2 + l) else { break };
        v.push((id, body));
        i += 2 + l;
    }
    v
}

/// IEs of a beacon / probe response (after the 12 fixed bytes).
pub fn beacon_ies(f: &[u8]) -> Vec<(u8, &[u8])> {
    f.get(36..).map(ies).unwrap_or_default()
}

pub fn ssid(ies: &[(u8, &[u8])]) -> Option<String> {
    ies.iter().find(|(id, _)| *id == 0).map(|(_, b)| String::from_utf8_lossy(b).chars().filter(|c| !c.is_control()).collect())
}

pub const WFA_OUI: [u8; 3] = [0x50, 0x6F, 0x9A];
pub const NAN_OUI_TYPE: u8 = 0x13;
pub const NAN_ATTR_SDA: u8 = 0x03;

/// NAN Service Discovery Frame (public action, vendor specific, WFA NAN):
/// every Service Descriptor Attribute as (service id, service info).
pub fn nan_sdas(f: &[u8]) -> Vec<([u8; 6], &[u8])> {
    let mut out = Vec::new();
    // 24 header + category 0x04 + action 0x09 + OUI + type
    if f.len() < 30 || f[24] != 0x04 || f[25] != 0x09 || f[26..29] != WFA_OUI || f[29] != NAN_OUI_TYPE {
        return out;
    }
    let mut i = 30usize;
    while i + 3 <= f.len() {
        let id = f[i];
        let l = u16::from_le_bytes([f[i + 1], f[i + 2]]) as usize;
        let Some(a) = f.get(i + 3..i + 3 + l) else { break };
        if id == NAN_ATTR_SDA && a.len() >= 9 {
            let mut sid = [0u8; 6];
            sid.copy_from_slice(&a[0..6]);
            let ctrl = a[8];
            let mut j = 9usize;
            if ctrl & 0x40 != 0 {
                j += 2; // binding bitmap
            }
            let mut ok = true;
            for bit in [0x04u8, 0x08] {
                if ctrl & bit != 0 {
                    match a.get(j) {
                        Some(&n) => j += 1 + n as usize,
                        None => ok = false,
                    }
                }
            }
            if ok && ctrl & 0x10 != 0 {
                if let Some(&n) = a.get(j) {
                    if let Some(si) = a.get(j + 1..j + 1 + n as usize) {
                        out.push((sid, si));
                    }
                }
            }
        }
        i += 3 + l;
    }
    out
}
