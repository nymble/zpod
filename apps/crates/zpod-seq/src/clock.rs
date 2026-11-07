//! Transport: pattern → sample-accurate events, block by block.
//!
//! Step `n` (absolute, counted since Play) starts at
//! `anchor_frame + (n - anchor_step) * step_len + swing(n)` frames, computed
//! from the index every time, so long runs do not drift. A tempo change
//! re-anchors at the next step. Each note-on schedules its note-off
//! (`gate` quarter steps later); `stop` ends every scheduled note.

use crate::event::{Event, EventBuf, EventKind};
use crate::pattern::{Pattern, TRACKS};

pub const OFF_CAP: usize = 48;

#[derive(Clone, Copy, Debug)]
struct Off {
    due: u64,
    track: u8,
    id: u32,
}

#[derive(Clone, Debug)]
pub struct Clock {
    sr: f64,
    playing: bool,
    pos: u64,
    anchor_frame: f64,
    anchor_step: u64,
    step_len: f64,
    tempo: f32,
    next_step: u64,
    offs: [Option<Off>; OFF_CAP],
    next_id: u32,
    /// pattern step that fired last (the playhead), None when stopped
    pub current_step: Option<u8>,
    /// absolute steps fired since Play
    pub steps_played: u64,
}

/// Frames per 1/16 step.
pub fn step_frames(sr: f64, bpm: f32) -> f64 {
    sr * 60.0 / (bpm as f64 * 4.0)
}

fn hash(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^ (x >> 16)
}

impl Clock {
    pub fn new(sample_rate: u32) -> Self {
        Clock {
            sr: sample_rate as f64,
            playing: false,
            pos: 0,
            anchor_frame: 0.0,
            anchor_step: 0,
            step_len: step_frames(sample_rate as f64, 100.0),
            tempo: 100.0,
            next_step: 0,
            offs: [None; OFF_CAP],
            next_id: 1,
            current_step: None,
            steps_played: 0,
        }
    }

    pub fn playing(&self) -> bool {
        self.playing
    }

    pub fn held(&self) -> usize {
        self.offs.iter().filter(|o| o.is_some()).count()
    }

    /// Play from step 1 (predictable: always the start of the pattern).
    pub fn start(&mut self, pat: &Pattern) {
        self.playing = true;
        self.pos = 0;
        self.anchor_frame = 0.0;
        self.anchor_step = 0;
        self.tempo = pat.tempo;
        self.step_len = step_frames(self.sr, pat.tempo);
        self.next_step = 0;
        self.steps_played = 0;
    }

    /// Stop: note-offs for everything scheduled + AllNotesOff, at offset 0.
    pub fn stop(&mut self, out: &mut EventBuf) {
        for o in self.offs.iter_mut() {
            if let Some(x) = o.take() {
                out.push(Event { offset: 0, kind: EventKind::NoteOff { track: x.track, id: x.id } });
            }
        }
        out.push(Event { offset: 0, kind: EventKind::AllNotesOff });
        self.playing = false;
        self.current_step = None;
    }

    fn step_time(&self, n: u64, pat: &Pattern) -> u64 {
        let straight = self.anchor_frame + (n - self.anchor_step) as f64 * self.step_len;
        let idx = n % pat.length.max(1) as u64;
        let swing = if idx % 2 == 1 { (pat.swing as f64 / 50.0 - 1.0) * self.step_len } else { 0.0 };
        (straight + swing).round() as u64
    }

    /// Events for the next `frames` frames (appended to `out`, sorted).
    pub fn process(&mut self, pat: &Pattern, frames: u32, out: &mut EventBuf) {
        if !self.playing {
            return;
        }
        if pat.tempo != self.tempo {
            self.anchor_frame += (self.next_step - self.anchor_step) as f64 * self.step_len;
            self.anchor_step = self.next_step;
            self.tempo = pat.tempo;
            self.step_len = step_frames(self.sr, pat.tempo);
        }
        let end = self.pos + frames as u64;
        loop {
            let t = self.step_time(self.next_step, pat);
            if t >= end {
                break;
            }
            self.fire(pat, t, out);
            self.next_step += 1;
        }
        for o in self.offs.iter_mut() {
            if let Some(x) = *o {
                if x.due < end {
                    out.push(Event { offset: x.due.saturating_sub(self.pos) as u32, kind: EventKind::NoteOff { track: x.track, id: x.id } });
                    *o = None;
                }
            }
        }
        out.sort();
        self.pos = end;
    }

    fn fire(&mut self, pat: &Pattern, t: u64, out: &mut EventBuf) {
        let len = pat.length.max(1) as u64;
        let n = self.next_step;
        let idx = (n % len) as usize;
        let bar = (n / len) as u32;
        let offset = t.saturating_sub(self.pos) as u32;
        self.current_step = Some(idx as u8);
        self.steps_played += 1;
        for tr in 0..TRACKS {
            let track = &pat.tracks[tr];
            let s = track.steps[idx];
            if track.mute || !s.on || s.vel == 0 {
                continue;
            }
            if s.prob < 100 {
                let h = hash(pat.seed ^ hash(bar.wrapping_mul(0x9E37_79B9) ^ ((tr as u32) << 8) ^ idx as u32)) % 100;
                if h >= s.prob as u32 {
                    continue;
                }
            }
            let id = self.next_id;
            self.next_id = self.next_id.wrapping_add(1).max(1);
            let gate = ((s.gate as f64 / 4.0) * self.step_len).max(64.0) as u64;
            let off = Off { due: t.max(self.pos) + gate, track: tr as u8, id };
            let slot = match self.offs.iter().position(|o| o.is_none()) {
                Some(i) => i,
                None => {
                    // full: end the earliest note now (never leave one hanging)
                    let i = (0..OFF_CAP).min_by_key(|&i| self.offs[i].map(|o| o.due).unwrap_or(u64::MAX)).unwrap_or(0);
                    if let Some(x) = self.offs[i] {
                        out.push(Event { offset, kind: EventKind::NoteOff { track: x.track, id: x.id } });
                    }
                    i
                }
            };
            self.offs[slot] = Some(off);
            out.push(Event { offset, kind: EventKind::NoteOn { track: tr as u8, id, pitch: pat.pitch(tr, &s), vel: s.vel as f32 / 127.0 } });
        }
    }
}
