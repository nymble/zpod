//! Render side of the synth: MIDI input messages -> zpod-voices rack
//! (audition slot = one selectable voice, up to the voice's polyphony).
//! `handle` and `render` do not allocate or lock.

use zpod_midi::input::InMsg;
use zpod_seq::event::{Event, EventBuf, EventKind, Pitch};
use zpod_seq::pattern::Macros;
use zpod_voices::{Bank, Rack, AUDITION, MAX_BLOCK};

pub struct Engine {
    pub rack: Rack,
    ev: EventBuf,
    buf: Vec<f32>,
    /// note -> sounding id (0 = off)
    ids: [u32; 128],
    next: u32,
    /// pitch bend in semitones applied to new notes (M1)
    pub bend: f32,
    pub bend_range: f32,
    pub voice: usize,
}

impl Engine {
    pub fn new(sr: u32, bank: Bank, voice: usize) -> Engine {
        let mut rack = Rack::new(sr, bank);
        rack.out_gain = 0.7;
        rack.set_audition(voice, Macros::default());
        Engine { rack, ev: EventBuf::new(), buf: vec![0.0; MAX_BLOCK * 2], ids: [0; 128], next: 0, bend: 0.0, bend_range: 2.0, voice }
    }

    pub fn set_voice(&mut self, v: usize) {
        self.all_off();
        if self.rack.set_audition(v, Macros::default()) {
            self.voice = v;
        }
    }

    fn all_off(&mut self) {
        self.ev.push(Event { offset: 0, kind: EventKind::AllNotesOff });
        self.ids = [0; 128];
    }

    #[allow(dead_code)]
    pub fn sounding(&self) -> usize {
        self.ids.iter().filter(|i| **i != 0).count()
    }

    pub fn handle(&mut self, m: InMsg) {
        let t = AUDITION as u8;
        match m {
            InMsg::NoteOn { note, vel, .. } => {
                let n = (note & 0x7f) as usize;
                if self.ids[n] != 0 {
                    self.ev.push(Event { offset: 0, kind: EventKind::NoteOff { track: t, id: self.ids[n] } });
                }
                self.next = self.next.wrapping_add(1).max(1);
                self.ids[n] = self.next;
                let pitch = Pitch::from_note(n as f32 + self.bend);
                self.ev.push(Event { offset: 0, kind: EventKind::NoteOn { track: t, id: self.next, pitch, vel: vel as f32 / 127.0 } });
            }
            InMsg::NoteOff { note, .. } => {
                let n = (note & 0x7f) as usize;
                if self.ids[n] != 0 {
                    self.ev.push(Event { offset: 0, kind: EventKind::NoteOff { track: t, id: self.ids[n] } });
                    self.ids[n] = 0;
                }
            }
            InMsg::PitchBend { val, .. } => self.bend = val as f32 / 8192.0 * self.bend_range,
            InMsg::BendRange { semis, cents, .. } => self.bend_range = (semis as f32 + cents as f32 / 100.0).clamp(0.0, 24.0),
            m if m.is_panic() => self.all_off(),
            InMsg::Stop => {}
            _ => {}
        }
    }

    /// One period of interleaved stereo i16.
    pub fn render(&mut self, out: &mut [i16], frames: usize) {
        let frames = frames.min(MAX_BLOCK).min(out.len() / 2);
        self.rack.render(self.ev.as_slice(), &mut self.buf, frames);
        self.ev.clear();
        for (o, x) in out.iter_mut().zip(self.buf[..frames * 2].iter()) {
            *o = (x.clamp(-1.0, 1.0) * 32767.0) as i16;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peak(e: &mut Engine, periods: usize) -> i16 {
        let mut out = vec![0i16; 512];
        let mut p = 0i16;
        for _ in 0..periods {
            e.render(&mut out, 256);
            p = p.max(out.iter().map(|x| x.saturating_abs()).max().unwrap_or(0));
        }
        p
    }

    fn engine() -> Engine {
        let bank = Bank::builtin();
        let v = (0..bank.voices.len()).find(|&i| !bank.voices[i].kind.is_drum()).unwrap_or(0);
        Engine::new(48_000, bank, v)
    }

    #[test]
    fn note_sounds_and_releases() {
        let mut e = engine();
        assert_eq!(peak(&mut e, 4), 0, "silent before any note");
        e.handle(InMsg::NoteOn { ch: 1, note: 60, vel: 110 });
        assert!(peak(&mut e, 20) > 1000, "a note must be audible");
        assert_eq!(e.sounding(), 1);
        e.handle(InMsg::NoteOff { ch: 1, note: 60, vel: 64 });
        assert_eq!(e.sounding(), 0);
        let _ = peak(&mut e, 400); // ~2 s of release
        assert!(peak(&mut e, 4) < 50, "release must end");
    }

    #[test]
    fn panic_silences_everything() {
        let mut e = engine();
        for n in [60u8, 64, 67] {
            e.handle(InMsg::NoteOn { ch: 1, note: n, vel: 100 });
        }
        let _ = peak(&mut e, 10);
        e.handle(InMsg::Cc { ch: 1, cc: 123, val: 0 });
        assert_eq!(e.sounding(), 0);
        let _ = peak(&mut e, 400);
        assert!(peak(&mut e, 4) < 50);
    }

    #[test]
    fn bend_and_voice_change() {
        let mut e = engine();
        e.handle(InMsg::BendRange { ch: 1, semis: 12, cents: 0 });
        e.handle(InMsg::PitchBend { ch: 1, val: 8191 });
        assert!((e.bend - 12.0).abs() < 0.01);
        e.handle(InMsg::NoteOn { ch: 1, note: 60, vel: 100 });
        let next = (e.voice + 1) % e.rack.bank.voices.len();
        e.set_voice(next);
        assert_eq!((e.voice, e.sounding()), (next, 0));
        // retrigger of the same note does not leak ids
        e.handle(InMsg::NoteOn { ch: 1, note: 62, vel: 100 });
        e.handle(InMsg::NoteOn { ch: 1, note: 62, vel: 100 });
        assert_eq!(e.sounding(), 1);
    }
}
