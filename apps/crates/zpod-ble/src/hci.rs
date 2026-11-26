// Copyright (C) 2026 Paul Lambert
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! HCI monitor channel framing (Linux `hci_mon_hdr`) and LE advertising
//! report events (Core spec Vol 4 Part E 7.7.65.2 / 7.7.65.13).

/// Monitor opcode for an HCI event packet.
pub const MON_EVENT_PKT: u16 = 3;
pub const EVT_LE_META: u8 = 0x3E;
pub const LE_ADV_REPORT: u8 = 0x02;
pub const LE_EXT_ADV_REPORT: u8 = 0x0D;

#[derive(Clone, Debug, PartialEq)]
pub struct AdvReport {
    /// "AA:BB:CC:DD:EE:FF" (display order)
    pub addr: String,
    /// 0 public, 1 random, 2/3 resolved identity
    pub addr_type: u8,
    pub rssi: i8,
    /// legacy event type (0 ADV_IND .. 4 SCAN_RSP) or extended properties
    pub event_type: u16,
    pub extended: bool,
    pub tx_power: Option<i8>,
    pub data: Vec<u8>,
}

impl AdvReport {
    pub fn connectable(&self) -> bool {
        if self.extended {
            self.event_type & 0x01 != 0
        } else {
            matches!(self.event_type, 0 | 1)
        }
    }
    pub fn scan_response(&self) -> bool {
        if self.extended {
            self.event_type & 0x08 != 0
        } else {
            self.event_type == 4
        }
    }
    /// Random address subtype from the top two bits.
    pub fn addr_kind(&self) -> &'static str {
        if self.addr_type & 1 == 0 {
            return "public";
        }
        let top = u8::from_str_radix(self.addr.get(0..2).unwrap_or("00"), 16).unwrap_or(0) >> 6;
        match top {
            0b11 => "random static",
            0b01 => "resolvable private",
            0b00 => "non-resolvable",
            _ => "random",
        }
    }
}

fn addr(b: &[u8]) -> String {
    format!("{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}", b[5], b[4], b[3], b[2], b[1], b[0])
}

/// One monitor-channel packet: header (opcode, index, len) + payload.
/// Returns the advertising reports in it (empty for anything else).
pub fn parse_monitor(pkt: &[u8]) -> Vec<AdvReport> {
    if pkt.len() < 6 {
        return Vec::new();
    }
    let op = u16::from_le_bytes([pkt[0], pkt[1]]);
    let len = u16::from_le_bytes([pkt[4], pkt[5]]) as usize;
    if op != MON_EVENT_PKT {
        return Vec::new();
    }
    let end = (6 + len).min(pkt.len());
    parse_event(&pkt[6..end])
}

/// An HCI event (code, plen, params).
pub fn parse_event(ev: &[u8]) -> Vec<AdvReport> {
    let mut out = Vec::new();
    if ev.len() < 4 || ev[0] != EVT_LE_META {
        return out;
    }
    let plen = (ev[1] as usize).min(ev.len() - 2);
    let p = &ev[2..2 + plen];
    if p.len() < 2 {
        return out;
    }
    let n = p[1] as usize;
    let mut i = 2usize;
    match p[0] {
        LE_ADV_REPORT => {
            for _ in 0..n {
                // event_type, addr_type, addr[6], data_len, data, rssi
                if i + 9 > p.len() {
                    break;
                }
                let dl = p[i + 8] as usize;
                if i + 9 + dl + 1 > p.len() {
                    break;
                }
                out.push(AdvReport {
                    addr: addr(&p[i + 2..i + 8]),
                    addr_type: p[i + 1],
                    event_type: p[i] as u16,
                    extended: false,
                    tx_power: None,
                    data: p[i + 9..i + 9 + dl].to_vec(),
                    rssi: p[i + 9 + dl] as i8,
                });
                i += 10 + dl;
            }
        }
        LE_EXT_ADV_REPORT => {
            for _ in 0..n {
                if i + 24 > p.len() {
                    break;
                }
                let dl = p[i + 23] as usize;
                if i + 24 + dl > p.len() {
                    break;
                }
                let tx = p[i + 12] as i8;
                out.push(AdvReport {
                    event_type: u16::from_le_bytes([p[i], p[i + 1]]),
                    addr_type: p[i + 2],
                    addr: addr(&p[i + 3..i + 9]),
                    extended: true,
                    tx_power: (tx != 127).then_some(tx),
                    rssi: p[i + 13] as i8,
                    data: p[i + 24..i + 24 + dl].to_vec(),
                });
                i += 24 + dl;
            }
        }
        _ => {}
    }
    out
}
