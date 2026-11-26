// Copyright (C) 2026 Paul Lambert
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! OFFLINE test pcap generator: a labelled TEST aircraft
//! ([`zpod_remoteid::SimAircraft`]) as radiotap + Wi-Fi beacon frames with
//! the Remote ID vendor IE, written to a pcap file in memory or on disk.
//! For decoder, Wireshark and map tests on a computer. No radio is used and
//! nothing in the ZPOD image can transmit these frames.

use std::io::{self, Write};

use crate::pcap;
use zpod_remoteid::encode::{self, TEST_MAC};
use zpod_remoteid::SimAircraft;

/// SSID of every generated beacon.
pub const TEST_SSID: &str = "ZPOD-TEST-RID";

fn chan_freq(ch: u32) -> u16 {
    match ch {
        14 => 2484,
        1..=13 => (2407 + 5 * ch) as u16,
        _ => (5000 + 5 * ch) as u16,
    }
}

/// radiotap (flags, channel) + beacon frame carrying `ie` (Remote ID IE body).
pub fn beacon_frame(ch: u32, seq: u16, ie: &[u8]) -> Vec<u8> {
    let present: u32 = (1 << 1) | (1 << 3);
    let mut v = vec![0, 0, 0, 0];
    v.extend_from_slice(&present.to_le_bytes());
    v.push(0); // flags: no FCS
    v.push(0); // align channel to 2
    v.extend_from_slice(&chan_freq(ch).to_le_bytes());
    v.extend_from_slice(&(if ch > 14 { 0x0140u16 } else { 0x00a0u16 }).to_le_bytes());
    let l = v.len() as u16;
    v[2..4].copy_from_slice(&l.to_le_bytes());
    v.extend_from_slice(&[0x80, 0, 0, 0]);
    v.extend_from_slice(&[0xff; 6]);
    v.extend_from_slice(&TEST_MAC);
    v.extend_from_slice(&TEST_MAC);
    v.extend_from_slice(&(seq << 4).to_le_bytes());
    v.extend_from_slice(&[0u8; 8]); // timestamp
    v.extend_from_slice(&100u16.to_le_bytes()); // interval
    v.extend_from_slice(&0x0001u16.to_le_bytes()); // ESS
    v.push(0);
    v.push(TEST_SSID.len() as u8);
    v.extend_from_slice(TEST_SSID.as_bytes());
    v.push(221);
    v.push(ie.len() as u8);
    v.extend_from_slice(ie);
    v
}

/// Write `count` beacons, `interval_s` apart, of `a` moving along its
/// track, on channel `ch`. Returns the number of frames written.
pub fn write_track_pcap<W: Write>(w: W, a: &mut SimAircraft, ch: u32, count: u32, interval_s: f64, start_us: u64) -> io::Result<u32> {
    let mut p = pcap::Writer::new(w, pcap::LINKTYPE_RADIOTAP)?;
    for i in 0..count {
        let t_us = start_us + (a.t_s * 1e6) as u64;
        let secs = t_us / 1_000_000;
        let tenths = (((secs % 3600) * 10) + (t_us % 1_000_000) / 100_000) as u16;
        let ts2019 = secs.saturating_sub(1_546_300_800) as u32;
        let ie = encode::beacon_ie(i as u8, &encode::pack(&a.messages(tenths, ts2019)));
        p.packet(t_us, &beacon_frame(ch, i as u16 & 0x0fff, &ie))?;
        a.step(interval_s);
    }
    p.flush()?;
    Ok(count)
}
