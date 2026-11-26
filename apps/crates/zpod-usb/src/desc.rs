// Copyright (C) 2026 Paul Lambert
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! Raw USB descriptors (USB 2.0 spec chapter 9) as found in sysfs
//! `descriptors`: the 18-byte device descriptor, then configuration
//! descriptor sets.

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DeviceDesc {
    pub bcd_usb: u16,
    pub class: u8,
    pub subclass: u8,
    pub protocol: u8,
    pub max_packet0: u8,
    pub vid: u16,
    pub pid: u16,
    pub bcd_device: u16,
    pub num_configs: u8,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Endpoint {
    pub address: u8,
    pub attributes: u8,
    pub max_packet: u16,
    pub interval: u8,
}

impl Endpoint {
    pub fn dir_in(&self) -> bool {
        self.address & 0x80 != 0
    }
    pub fn kind(&self) -> &'static str {
        match self.attributes & 3 {
            0 => "control",
            1 => "iso",
            2 => "bulk",
            _ => "interrupt",
        }
    }
    pub fn describe(&self) -> String {
        format!("EP {:02x} {} {} {}B int {}", self.address, if self.dir_in() { "IN" } else { "OUT" }, self.kind(), self.max_packet & 0x7ff, self.interval)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct IfaceDesc {
    pub number: u8,
    pub alt: u8,
    pub class: u8,
    pub subclass: u8,
    pub protocol: u8,
    pub endpoints: Vec<Endpoint>,
    /// HID class descriptor: report descriptor length
    pub hid_report_len: Option<u16>,
    /// other class-specific descriptors: (type, length)
    pub extra: Vec<(u8, u8)>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConfigDesc {
    pub value: u8,
    pub attributes: u8,
    /// mA
    pub max_power_ma: u16,
    pub interfaces: Vec<IfaceDesc>,
    /// interface association descriptors: (first iface, count, function class)
    pub associations: Vec<(u8, u8, u8)>,
}

impl ConfigDesc {
    pub fn self_powered(&self) -> bool {
        self.attributes & 0x40 != 0
    }
    pub fn remote_wakeup(&self) -> bool {
        self.attributes & 0x20 != 0
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Parsed {
    pub device: Option<DeviceDesc>,
    pub configs: Vec<ConfigDesc>,
    /// descriptors that were cut short or inconsistent
    pub warnings: Vec<String>,
}

fn le16(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}

/// Parse a sysfs `descriptors` blob (or any device + config descriptor
/// sequence). Never panics; bad lengths stop the walk with a warning.
pub fn parse(b: &[u8]) -> Parsed {
    let mut p = Parsed::default();
    let mut i = 0usize;
    while i + 2 <= b.len() {
        let len = b[i] as usize;
        let ty = b[i + 1];
        if len < 2 || i + len > b.len() {
            p.warnings.push(format!("bad descriptor length {len} at offset {i}"));
            break;
        }
        let d = &b[i..i + len];
        match ty {
            1 if len >= 18 => {
                p.device = Some(DeviceDesc {
                    bcd_usb: le16(d, 2),
                    class: d[4],
                    subclass: d[5],
                    protocol: d[6],
                    max_packet0: d[7],
                    vid: le16(d, 8),
                    pid: le16(d, 10),
                    bcd_device: le16(d, 12),
                    num_configs: d[17],
                });
            }
            2 if len >= 9 => p.configs.push(ConfigDesc { value: d[5], attributes: d[7], max_power_ma: d[8] as u16 * 2, ..Default::default() }),
            4 if len >= 9 => {
                if let Some(c) = p.configs.last_mut() {
                    c.interfaces.push(IfaceDesc { number: d[2], alt: d[3], class: d[5], subclass: d[6], protocol: d[7], ..Default::default() });
                } else {
                    p.warnings.push("interface before configuration".into());
                }
            }
            5 if len >= 7 => {
                if let Some(it) = p.configs.last_mut().and_then(|c| c.interfaces.last_mut()) {
                    it.endpoints.push(Endpoint { address: d[2], attributes: d[3], max_packet: le16(d, 4), interval: d[6] });
                }
            }
            0x0B if len >= 8 => {
                if let Some(c) = p.configs.last_mut() {
                    c.associations.push((d[2], d[3], d[4]));
                }
            }
            0x21 if len >= 9 => {
                if let Some(it) = p.configs.last_mut().and_then(|c| c.interfaces.last_mut()) {
                    if it.class == 3 && d[6] == 0x22 {
                        it.hid_report_len = Some(le16(d, 7));
                    } else {
                        it.extra.push((ty, len as u8));
                    }
                }
            }
            _ => {
                if let Some(it) = p.configs.last_mut().and_then(|c| c.interfaces.last_mut()) {
                    if it.extra.len() < 16 {
                        it.extra.push((ty, len as u8));
                    }
                }
            }
        }
        i += len;
    }
    p
}
