//! MIDI 1.0 byte encoding. Channels are 1..=16 in the API, 0..=15 on the wire.

pub const NOTE_OFF: u8 = 0x80;
pub const NOTE_ON: u8 = 0x90;
pub const CC: u8 = 0xB0;
pub const BEND: u8 = 0xE0;
pub const CLOCK: u8 = 0xF8;
pub const START: u8 = 0xFA;
pub const STOP: u8 = 0xFC;
/// CC 123: All Notes Off (channel mode message).
pub const CC_ALL_NOTES_OFF: u8 = 123;
pub const BEND_CENTER: u16 = 8192;

/// One short MIDI message (1-3 bytes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Msg {
    pub len: u8,
    pub b: [u8; 3],
}

fn st(kind: u8, ch: u8) -> u8 {
    kind | (ch.clamp(1, 16) - 1)
}

impl Msg {
    pub const EMPTY: Msg = Msg { len: 0, b: [0; 3] };
    pub fn note_on(ch: u8, note: u8, vel: u8) -> Msg {
        Msg { len: 3, b: [st(NOTE_ON, ch), note & 0x7f, vel.clamp(1, 127)] }
    }
    pub fn note_off(ch: u8, note: u8) -> Msg {
        Msg { len: 3, b: [st(NOTE_OFF, ch), note & 0x7f, 0] }
    }
    pub fn cc(ch: u8, cc: u8, v: u8) -> Msg {
        Msg { len: 3, b: [st(CC, ch), cc & 0x7f, v & 0x7f] }
    }
    /// 14-bit pitch bend, 0..=16383, 8192 = centre. LSB first on the wire.
    pub fn bend(ch: u8, v: u16) -> Msg {
        let v = v.min(16383);
        Msg { len: 3, b: [st(BEND, ch), (v & 0x7f) as u8, (v >> 7) as u8] }
    }
    pub fn realtime(b: u8) -> Msg {
        Msg { len: 1, b: [b, 0, 0] }
    }
    pub fn bytes(&self) -> &[u8] {
        &self.b[..self.len as usize]
    }
    /// Channel 1..=16 of a channel message.
    pub fn channel(&self) -> Option<u8> {
        (self.len > 0 && self.b[0] >= 0x80 && self.b[0] < 0xF0).then(|| (self.b[0] & 0x0f) + 1)
    }
}

/// Bend value for a detune in cents, given the receiver's bend range in
/// semitones (MIDI has no way to query it; the user sets it to match).
/// Out-of-range detunes saturate.
pub fn bend_value(cents: f32, range_semitones: u8) -> u16 {
    let r = range_semitones.max(1) as f32 * 100.0;
    (BEND_CENTER as f32 + cents / r * 8192.0).round().clamp(0.0, 16383.0) as u16
}

/// 0..1 velocity to 1..=127.
pub fn vel7(v: f32) -> u8 {
    if v.is_finite() { (v * 127.0).round().clamp(1.0, 127.0) as u8 } else { 100 }
}
