// Copyright (C) 2026 Paul Lambert
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! MIDI 1.0 *input*: a byte-stream parser (running status, real-time bytes
//! interleaved anywhere, SysEx skipped, note-on velocity 0 = note-off) and
//! a non-blocking ALSA rawmidi reader. The parser is pure and
//! allocation-free, so it can also run on an audio thread.
//!
//! Groundwork for later milestones is parsed but not acted on by the M1
//! synth: RPN 0 (pitch-bend range), program change, pitch bend and
//! channel / poly pressure.

use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::path::Path;

/// Channels are 1..=16.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InMsg {
    NoteOn { ch: u8, note: u8, vel: u8 },
    NoteOff { ch: u8, note: u8, vel: u8 },
    PolyPressure { ch: u8, note: u8, val: u8 },
    Cc { ch: u8, cc: u8, val: u8 },
    Program { ch: u8, prog: u8 },
    ChanPressure { ch: u8, val: u8 },
    /// -8192..=8191, 0 = centre
    PitchBend { ch: u8, val: i16 },
    /// RPN 0 complete (after data entry MSB/LSB): semitones + cents
    BendRange { ch: u8, semis: u8, cents: u8 },
    Clock,
    Start,
    Continue,
    Stop,
    ActiveSensing,
    Reset,
}

impl InMsg {
    pub fn channel(&self) -> Option<u8> {
        match *self {
            InMsg::NoteOn { ch, .. } | InMsg::NoteOff { ch, .. } | InMsg::PolyPressure { ch, .. } | InMsg::Cc { ch, .. } | InMsg::Program { ch, .. } | InMsg::ChanPressure { ch, .. } | InMsg::PitchBend { ch, .. } | InMsg::BendRange { ch, .. } => Some(ch),
            _ => None,
        }
    }
    /// CC 120 All Sound Off, CC 123 All Notes Off, or System Reset.
    pub fn is_panic(&self) -> bool {
        matches!(self, InMsg::Cc { cc: 120 | 123, .. } | InMsg::Reset)
    }
}

/// Streaming parser state.
#[derive(Clone, Debug)]
pub struct Parser {
    status: u8,
    data: [u8; 2],
    have: usize,
    in_sysex: bool,
    /// per channel: (rpn msb, rpn lsb, data msb)
    rpn: [(u8, u8, u8); 16],
    pub bytes: u64,
    pub dropped: u64,
}

impl Default for Parser {
    fn default() -> Self {
        Parser { status: 0, data: [0; 2], have: 0, in_sysex: false, rpn: [(127, 127, 0); 16], bytes: 0, dropped: 0 }
    }
}

fn data_len(status: u8) -> usize {
    match status & 0xF0 {
        0xC0 | 0xD0 => 1,
        _ => 2,
    }
}

impl Parser {
    pub fn new() -> Parser {
        Parser::default()
    }

    /// Feed one byte; returns a message when one completes. Under RPN 0 a
    /// CC 6 / 38 data entry is returned as `BendRange` instead of the CC.
    pub fn feed(&mut self, b: u8) -> Option<InMsg> {
        self.bytes += 1;
        if b >= 0xF8 {
            // real-time: never disturbs running status or sysex
            return match b {
                0xF8 => Some(InMsg::Clock),
                0xFA => Some(InMsg::Start),
                0xFB => Some(InMsg::Continue),
                0xFC => Some(InMsg::Stop),
                0xFE => Some(InMsg::ActiveSensing),
                0xFF => {
                    self.status = 0;
                    self.have = 0;
                    self.in_sysex = false;
                    Some(InMsg::Reset)
                }
                _ => None,
            };
        }
        if b & 0x80 != 0 {
            if self.have > 0 {
                self.dropped += 1; // incomplete message cut short
            }
            self.have = 0;
            match b {
                0xF0 => {
                    self.in_sysex = true;
                    self.status = 0;
                }
                0xF7 => self.in_sysex = false,
                0xF1..=0xF6 => {
                    // system common: clears running status; data bytes ignored
                    self.in_sysex = false;
                    self.status = 0;
                }
                _ => {
                    self.in_sysex = false;
                    self.status = b;
                }
            }
            return None;
        }
        if self.in_sysex || self.status == 0 {
            if !self.in_sysex {
                self.dropped += 1;
            }
            return None;
        }
        self.data[self.have] = b;
        self.have += 1;
        if self.have < data_len(self.status) {
            return None;
        }
        self.have = 0;
        let ch = (self.status & 0x0F) + 1;
        let (d0, d1) = (self.data[0], self.data[1]);
        Some(match self.status & 0xF0 {
            0x80 => InMsg::NoteOff { ch, note: d0, vel: d1 },
            0x90 if d1 == 0 => InMsg::NoteOff { ch, note: d0, vel: 64 },
            0x90 => InMsg::NoteOn { ch, note: d0, vel: d1 },
            0xA0 => InMsg::PolyPressure { ch, note: d0, val: d1 },
            0xB0 => return Some(self.cc(ch, d0, d1)),
            0xC0 => InMsg::Program { ch, prog: d0 },
            0xD0 => InMsg::ChanPressure { ch, val: d0 },
            _ => InMsg::PitchBend { ch, val: (((d1 as i16) << 7) | d0 as i16) - 8192 },
        })
    }

    fn cc(&mut self, ch: u8, cc: u8, val: u8) -> InMsg {
        let r = &mut self.rpn[(ch - 1) as usize];
        match cc {
            101 => r.0 = val,
            100 => r.1 = val,
            6 if r.0 == 0 && r.1 == 0 => {
                r.2 = val;
                return InMsg::BendRange { ch, semis: val, cents: 0 };
            }
            38 if r.0 == 0 && r.1 == 0 => return InMsg::BendRange { ch, semis: r.2, cents: val },
            _ => {}
        }
        InMsg::Cc { ch, cc, val }
    }

    /// Parse a buffer, calling `f` for each message.
    pub fn feed_all(&mut self, bytes: &[u8], mut f: impl FnMut(InMsg)) {
        for &b in bytes {
            if let Some(m) = self.feed(b) {
                f(m);
            }
        }
    }
}

/// Channel filter: 0 = omni, 1..=16 = that channel only.
pub fn channel_ok(filter: u8, m: &InMsg) -> bool {
    filter == 0 || m.channel().is_none_or(|c| c == filter)
}

/// A rawmidi port opened read-only and non-blocking.
pub struct RawMidiIn {
    fd: OwnedFd,
    pub name: String,
    pub parser: Parser,
}

impl RawMidiIn {
    pub fn open(path: &Path, name: &str) -> io::Result<RawMidiIn> {
        use std::os::unix::ffi::OsStrExt;
        let c = std::ffi::CString::new(path.as_os_str().as_bytes()).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        // SAFETY: open(2) on a path we own a CString for
        let fd = unsafe { libc::open(c.as_ptr(), libc::O_RDONLY | libc::O_NONBLOCK | libc::O_CLOEXEC) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: fresh descriptor we own
        Ok(RawMidiIn { fd: unsafe { OwnedFd::from_raw_fd(fd) }, name: name.to_string(), parser: Parser::new() })
    }

    pub fn raw_fd(&self) -> i32 {
        self.fd.as_raw_fd()
    }

    /// Read what is there now; Ok(0) = nothing; Err = port gone.
    pub fn read_some(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        // SAFETY: read(2) into our buffer
        let n = unsafe { libc::read(self.fd.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
        if n >= 0 {
            if n == 0 {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "port closed"));
            }
            return Ok(n as usize);
        }
        let e = io::Error::last_os_error();
        match e.raw_os_error() {
            Some(libc::EAGAIN) | Some(libc::EINTR) => Ok(0),
            _ => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all(b: &[u8]) -> Vec<InMsg> {
        let mut p = Parser::new();
        let mut v = Vec::new();
        p.feed_all(b, |m| v.push(m));
        v
    }

    #[test]
    fn notes_and_running_status() {
        let v = all(&[0x90, 60, 100, 62, 90, 60, 0, 0x80, 62, 10]);
        assert_eq!(v, vec![InMsg::NoteOn { ch: 1, note: 60, vel: 100 }, InMsg::NoteOn { ch: 1, note: 62, vel: 90 }, InMsg::NoteOff { ch: 1, note: 60, vel: 64 }, InMsg::NoteOff { ch: 1, note: 62, vel: 10 }]);
    }

    #[test]
    fn realtime_inside_messages_and_sysex() {
        let v = all(&[0x93, 0xF8, 64, 0xFE, 127, 0xF0, 1, 2, 3, 0xF8, 0xF7, 64, 1]);
        assert_eq!(v, vec![InMsg::Clock, InMsg::ActiveSensing, InMsg::NoteOn { ch: 4, note: 64, vel: 127 }, InMsg::Clock]);
        // after sysex, running status is gone: stray data is dropped
        let mut p = Parser::new();
        p.feed_all(&[0xF0, 1, 0xF7, 60, 100], |_| panic!("no message expected"));
        assert_eq!(p.dropped, 2);
    }

    #[test]
    fn controllers_panic_and_groundwork() {
        let v = all(&[0xB0, 123, 0, 0xB1, 120, 0, 0xC2, 5, 0xD3, 77, 0xE0, 0, 64, 0xE0, 0, 0, 0xE0, 127, 127, 0xA5, 60, 30]);
        assert!(v[0].is_panic() && v[1].is_panic());
        assert_eq!(v[2], InMsg::Program { ch: 3, prog: 5 });
        assert_eq!(v[3], InMsg::ChanPressure { ch: 4, val: 77 });
        assert_eq!(v[4], InMsg::PitchBend { ch: 1, val: 0 });
        assert_eq!(v[5], InMsg::PitchBend { ch: 1, val: -8192 });
        assert_eq!(v[6], InMsg::PitchBend { ch: 1, val: 8191 });
        assert_eq!(v[7], InMsg::PolyPressure { ch: 6, note: 60, val: 30 });
        assert!(all(&[0xFF])[0].is_panic());
    }

    #[test]
    fn rpn0_bend_range() {
        let v = all(&[0xB0, 101, 0, 100, 0, 6, 12, 38, 50, 101, 127, 100, 127, 6, 3]);
        assert!(v.contains(&InMsg::BendRange { ch: 1, semis: 12, cents: 0 }));
        assert!(v.contains(&InMsg::BendRange { ch: 1, semis: 12, cents: 50 }));
        // after RPN null, CC 6 is a plain controller
        assert_eq!(*v.last().unwrap(), InMsg::Cc { ch: 1, cc: 6, val: 3 });
    }

    #[test]
    fn channel_filter() {
        let on = InMsg::NoteOn { ch: 2, note: 1, vel: 1 };
        assert!(channel_ok(0, &on) && channel_ok(2, &on) && !channel_ok(3, &on));
        assert!(channel_ok(5, &InMsg::Clock));
    }

    #[test]
    fn garbage_never_panics() {
        let mut p = Parser::new();
        let mut x: u32 = 7;
        for _ in 0..20_000 {
            x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let _ = p.feed((x >> 24) as u8);
        }
        assert_eq!(p.bytes, 20_000);
    }

    #[test]
    fn fifo_reader() {
        // a regular file stands in for a port: reads, then EOF = gone
        let p = std::env::temp_dir().join(format!("zpod-midi-in-{}", std::process::id()));
        std::fs::write(&p, [0x90u8, 60, 100]).unwrap();
        let mut r = RawMidiIn::open(&p, "test").unwrap();
        let mut b = [0u8; 16];
        let n = r.read_some(&mut b).unwrap();
        assert_eq!(n, 3);
        let mut got = None;
        r.parser.feed_all(&b[..n], |m| got = Some(m));
        assert_eq!(got, Some(InMsg::NoteOn { ch: 1, note: 60, vel: 100 }));
        assert!(r.read_some(&mut b).is_err());
        std::fs::remove_file(&p).ok();
    }
}
