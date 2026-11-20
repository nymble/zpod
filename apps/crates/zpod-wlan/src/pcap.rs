//! Classic libpcap files (no libpcap needed): write captures to /data and
//! read them back for tests and offline review in Wireshark.

use std::io::{self, Read, Write};

pub const MAGIC: u32 = 0xa1b2_c3d4;
/// LINKTYPE_IEEE802_11_RADIOTAP
pub const LINKTYPE_RADIOTAP: u32 = 127;
pub const SNAPLEN: u32 = 4096;

pub struct Writer<W: Write> {
    w: W,
    pub packets: u64,
    pub bytes: u64,
}

impl<W: Write> Writer<W> {
    pub fn new(mut w: W, linktype: u32) -> io::Result<Self> {
        let mut h = Vec::with_capacity(24);
        h.extend_from_slice(&MAGIC.to_le_bytes());
        h.extend_from_slice(&2u16.to_le_bytes());
        h.extend_from_slice(&4u16.to_le_bytes());
        h.extend_from_slice(&0i32.to_le_bytes());
        h.extend_from_slice(&0u32.to_le_bytes());
        h.extend_from_slice(&SNAPLEN.to_le_bytes());
        h.extend_from_slice(&linktype.to_le_bytes());
        w.write_all(&h)?;
        Ok(Writer { w, packets: 0, bytes: 24 })
    }
    pub fn packet(&mut self, ts_us: u64, data: &[u8]) -> io::Result<()> {
        let cap = data.len().min(SNAPLEN as usize);
        let mut h = Vec::with_capacity(16);
        h.extend_from_slice(&((ts_us / 1_000_000) as u32).to_le_bytes());
        h.extend_from_slice(&((ts_us % 1_000_000) as u32).to_le_bytes());
        h.extend_from_slice(&(cap as u32).to_le_bytes());
        h.extend_from_slice(&(data.len() as u32).to_le_bytes());
        self.w.write_all(&h)?;
        self.w.write_all(&data[..cap])?;
        self.packets += 1;
        self.bytes += 16 + cap as u64;
        Ok(())
    }
    pub fn flush(&mut self) -> io::Result<()> {
        self.w.flush()
    }
    pub fn into_inner(self) -> W {
        self.w
    }
}

/// Read every packet of a little-endian microsecond pcap: (linktype, [(ts_us, bytes)]).
pub fn read_all<R: Read>(mut r: R) -> io::Result<(u32, Vec<(u64, Vec<u8>)>)> {
    let mut b = Vec::new();
    r.read_to_end(&mut b)?;
    let bad = |m: &str| io::Error::new(io::ErrorKind::InvalidData, m.to_string());
    if b.len() < 24 || u32::from_le_bytes([b[0], b[1], b[2], b[3]]) != MAGIC {
        return Err(bad("not a little-endian pcap file"));
    }
    let lt = u32::from_le_bytes([b[20], b[21], b[22], b[23]]);
    let mut v = Vec::new();
    let mut i = 24usize;
    while i + 16 <= b.len() {
        let s = u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]) as u64;
        let us = u32::from_le_bytes([b[i + 4], b[i + 5], b[i + 6], b[i + 7]]) as u64;
        let n = u32::from_le_bytes([b[i + 8], b[i + 9], b[i + 10], b[i + 11]]) as usize;
        let Some(d) = b.get(i + 16..i + 16 + n) else { return Err(bad("truncated packet")) };
        v.push((s * 1_000_000 + us, d.to_vec()));
        i += 16 + n;
    }
    if i != b.len() {
        return Err(bad("truncated record header"));
    }
    Ok((lt, v))
}
