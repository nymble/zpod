//! Render-thread side: app note events -> timed MIDI messages.
//!
//! No allocation, locks or IO; fixed tables only. Note lifetimes:
//! * each (channel, note) has at most one owner id. A note-on for a sounding
//!   (channel, note) is a retrigger: note-off first, then note-on, and the
//!   new id takes over;
//! * a note-off whose id no longer owns its note (it was retriggered) sends
//!   nothing, so overlapping notes never cut each other short;
//! * `panic` (routing change, AllOff, stop, exit) sends a note-off for every
//!   owned note, then CC 123 and bend centre on every used channel.
//!
//! Pitch: lanes with a fixed note (drum map) ignore the event pitch. Other
//! lanes send the nearest note number and put the remainder (cents) in a
//! per-channel pitch bend, sent only when it changes and before the note-on.
//! A bend applies to the whole channel, so this is exact when every note on
//! a channel has the same detune (in Pipit the detune is a per-lane macro and
//! each lane has its own channel); lanes that share a channel share the bend.

use crate::config::MidiConfig;
use crate::encode::{bend_value, vel7, Msg, BEND_CENTER, CC_ALL_NOTES_OFF, CLOCK, START, STOP};
use crate::queue::{Item, Ring};
use std::sync::Arc;

pub const OWNERS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum In {
    /// `note`: fractional MIDI note (60.25 = C4 + 25 cents); `vel` 0..1
    On { lane: u8, id: u32, note: f32, vel: f32 },
    Off { lane: u8, id: u32 },
    AllOff,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InEvent {
    /// frame inside the current block
    pub offset: u32,
    pub ev: In,
}

#[derive(Clone, Copy)]
struct Owner {
    id: u32,
    ch: u8,
    note: u8,
}

pub struct Router {
    ring: Arc<Ring>,
    cfg: MidiConfig,
    sr: f64,
    /// added to every due time: how far audio output lags the render
    pub latency_ns: u64,
    owners: [Option<Owner>; OWNERS],
    bend: [u16; 16],
    clock_on: bool,
    next_tick: f64, // frames since the clock start
    clock_pos: f64,
    pub overflow: u32,
}

impl Router {
    pub fn new(ring: Arc<Ring>, cfg: MidiConfig, sample_rate: u32, latency_ns: u64) -> Router {
        Router { ring, cfg, sr: sample_rate as f64, latency_ns, owners: [None; OWNERS], bend: [BEND_CENTER; 16], clock_on: false, next_tick: 0.0, clock_pos: 0.0, overflow: 0 }
    }
    pub fn config(&self) -> &MidiConfig {
        &self.cfg
    }
    pub fn sounding(&self) -> usize {
        self.owners.iter().flatten().count()
    }

    fn push(&mut self, due: u64, m: Msg) {
        if !self.ring.push(Item { due_ns: due, msg: m }) {
            self.overflow = self.overflow.saturating_add(1);
        }
    }

    /// Apply a new configuration. A change that affects sounding notes
    /// (route, port, lanes, bend range) panics with the old map first; turning
    /// the clock off (or routing to Internal) sends Stop if it was running.
    pub fn set_config(&mut self, now_ns: u64, cfg: MidiConfig) {
        let due = now_ns + self.latency_ns;
        let clock_was = self.clock_on && self.cfg.route.sends() && self.cfg.clock;
        if !self.cfg.same_routing(&cfg) {
            self.panic(now_ns);
        }
        if clock_was && !(cfg.clock && cfg.route.sends()) {
            self.push(due, Msg::realtime(STOP));
        }
        self.cfg = cfg;
    }

    /// End every note this router started and reset the used channels.
    pub fn panic(&mut self, now_ns: u64) {
        let due = now_ns + self.latency_ns;
        for i in 0..OWNERS {
            if let Some(o) = self.owners[i].take() {
                self.push(due, Msg::note_off(o.ch, o.note));
            }
        }
        if !self.cfg.route.sends() {
            self.bend = [BEND_CENTER; 16];
            return;
        }
        let mask = self.cfg.channel_mask();
        for ch in 1..=16u8 {
            if mask & (1 << (ch - 1)) != 0 {
                self.push(due, Msg::cc(ch, CC_ALL_NOTES_OFF, 0));
                if self.bend[ch as usize - 1] != BEND_CENTER {
                    self.push(due, Msg::bend(ch, BEND_CENTER));
                }
            }
        }
        self.bend = [BEND_CENTER; 16];
    }

    /// One audio block. `events` sorted by offset; `tempo` in bpm (clock);
    /// `playing` = transport state after this block's events.
    pub fn block(&mut self, now_ns: u64, frames: u32, events: &[InEvent], tempo: f32, playing: bool) {
        let sends = self.cfg.route.sends();
        let base = now_ns + self.latency_ns;
        let ns_per_frame = 1e9 / self.sr;
        let due = |off: u32| base + (off as f64 * ns_per_frame) as u64;
        // clock start comes before the first note of the block
        let clock = sends && self.cfg.clock;
        if clock && playing && !self.clock_on {
            self.clock_on = true;
            self.clock_pos = 0.0;
            self.next_tick = 0.0;
            self.push(due(0), Msg::realtime(START));
        }
        if sends {
            for e in events {
                let d = due(e.offset);
                match e.ev {
                    In::On { lane, id, note, vel } => {
                        let Some(l) = self.cfg.lanes().get(lane as usize).copied() else { continue };
                        let (n, cents) = match l.note {
                            Some(n) => (n, 0.0),
                            None => {
                                let r = note.round();
                                (r.clamp(0.0, 127.0) as u8, (note - r) * 100.0)
                            }
                        };
                        if l.note.is_none() {
                            let b = bend_value(cents, self.cfg.bend_range);
                            let ci = l.channel as usize - 1;
                            if b != self.bend[ci] {
                                self.bend[ci] = b;
                                self.push(d, Msg::bend(l.channel, b));
                            }
                        }
                        if let Some(o) = self.owners.iter_mut().flatten().find(|o| o.ch == l.channel && o.note == n) {
                            // retrigger: end the old one, the new id owns it
                            o.id = id;
                            self.push(d, Msg::note_off(l.channel, n));
                        } else if let Some(slot) = self.owners.iter_mut().find(|o| o.is_none()) {
                            *slot = Some(Owner { id, ch: l.channel, note: n });
                        } else {
                            // table full: skip the note rather than lose track of it
                            self.overflow = self.overflow.saturating_add(1);
                            continue;
                        }
                        self.push(d, Msg::note_on(l.channel, n, vel7(vel)));
                    }
                    In::Off { id, .. } => {
                        if let Some(slot) = self.owners.iter_mut().find(|o| o.map(|o| o.id) == Some(id)) {
                            let o = slot.take().unwrap();
                            self.push(d, Msg::note_off(o.ch, o.note));
                        }
                    }
                    In::AllOff => self.panic(d.saturating_sub(self.latency_ns)),
                }
            }
        }
        if self.clock_on {
            if !(clock && playing) {
                self.clock_on = false;
                self.push(due(frames), Msg::realtime(STOP));
            } else {
                // 24 pulses per quarter note; a tempo change alters the spacing
                // from the next pulse on, so pulses stay monotonic
                let fpt = self.sr * 60.0 / (tempo.clamp(20.0, 400.0) as f64 * 24.0);
                let end = self.clock_pos + frames as f64;
                while self.next_tick < end {
                    let off = (self.next_tick - self.clock_pos).max(0.0) as u32;
                    self.push(due(off), Msg::realtime(CLOCK));
                    self.next_tick += fpt;
                }
                self.clock_pos = end;
            }
        }
    }
}
