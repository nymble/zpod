// Copyright (C) 2026 Paul Lambert
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! Minimal radiotap header reader (radiotap.org): length, flags (FCS at
//! end), channel frequency and antenna signal.

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Radiotap {
    pub len: usize,
    pub flags: u8,
    pub freq: Option<u16>,
    pub signal: Option<i8>,
}

impl Radiotap {
    pub fn has_fcs(&self) -> bool {
        self.flags & 0x10 != 0
    }
    pub fn bad_fcs(&self) -> bool {
        self.flags & 0x40 != 0
    }
}

/// (alignment, size) of fields 0..=5 we walk through.
const FIELDS: [(usize, usize); 6] = [(8, 8), (1, 1), (1, 1), (2, 4), (1, 2), (1, 1)];

pub fn parse(b: &[u8]) -> Option<Radiotap> {
    if b.len() < 8 || b[0] != 0 {
        return None;
    }
    let len = u16::from_le_bytes([b[2], b[3]]) as usize;
    if len < 8 || len > b.len() {
        return None;
    }
    let present = u32::from_le_bytes([b[4], b[5], b[6], b[7]]);
    // skip extended present words
    let mut off = 8usize;
    let mut p = present;
    while p & 0x8000_0000 != 0 {
        if off + 4 > len {
            return None;
        }
        p = u32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]]);
        off += 4;
    }
    let mut r = Radiotap { len, ..Default::default() };
    for (bit, (align, size)) in FIELDS.iter().enumerate() {
        if present & (1 << bit) == 0 {
            continue;
        }
        off = off.div_ceil(*align) * *align;
        if off + size > len {
            return Some(r);
        }
        match bit {
            1 => r.flags = b[off],
            3 => r.freq = Some(u16::from_le_bytes([b[off], b[off + 1]])),
            5 => r.signal = Some(b[off] as i8),
            _ => {}
        }
        off += size;
    }
    Some(r)
}
