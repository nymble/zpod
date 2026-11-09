//! Events from a sequencer (or any other source) to a voice rack.

/// Absolute pitch. Hz is the source of truth; MIDI-style numbers are only a
/// convenience (fractional values = cents).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pitch {
    pub hz: f32,
}

impl Pitch {
    /// `note` in semitones, 69.0 = A4 = 440 Hz; 60.5 = C4 + 50 cents.
    pub fn from_note(note: f32) -> Self {
        Pitch { hz: 440.0 * libm_exp2((note - 69.0) / 12.0) }
    }
    pub fn note(self) -> f32 {
        69.0 + 12.0 * (self.hz / 440.0).log2()
    }
}

fn libm_exp2(x: f32) -> f32 {
    x.exp2()
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EventKind {
    /// Start a note on `track` (voice slot). `id` is unique per clock run and
    /// matches the later NoteOff.
    NoteOn { track: u8, id: u32, pitch: Pitch, vel: f32 },
    NoteOff { track: u8, id: u32 },
    /// Release everything (transport stop); voices fade, no hard cut.
    AllNotesOff,
}

/// `offset` = frame inside the block being rendered (0..frames).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Event {
    pub offset: u32,
    pub kind: EventKind,
}

pub const EVENT_CAP: usize = 128;

/// Fixed-capacity, caller-owned event list (no allocation while rendering).
#[derive(Clone, Debug)]
pub struct EventBuf {
    ev: [Event; EVENT_CAP],
    len: usize,
    pub dropped: u32,
}

impl Default for EventBuf {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBuf {
    pub const fn new() -> Self {
        EventBuf { ev: [Event { offset: 0, kind: EventKind::AllNotesOff }; EVENT_CAP], len: 0, dropped: 0 }
    }
    pub fn clear(&mut self) {
        self.len = 0;
    }
    pub fn push(&mut self, e: Event) -> bool {
        if self.len < EVENT_CAP {
            self.ev[self.len] = e;
            self.len += 1;
            true
        } else {
            self.dropped += 1;
            false
        }
    }
    pub fn as_slice(&self) -> &[Event] {
        &self.ev[..self.len]
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    /// Stable sort by offset; at equal offsets offs come before ons.
    pub fn sort(&mut self) {
        let s = &mut self.ev[..self.len];
        for i in 1..s.len() {
            let mut j = i;
            while j > 0 && key(&s[j - 1]) > key(&s[j]) {
                s.swap(j - 1, j);
                j -= 1;
            }
        }
        fn key(e: &Event) -> (u32, u8) {
            (e.offset, match e.kind { EventKind::AllNotesOff => 0, EventKind::NoteOff { .. } => 1, EventKind::NoteOn { .. } => 2 })
        }
    }
}
