//! Lightweight synth voices for the Pi Zero W (ARMv6, soft-float VFP).
//! One decent subtractive patch: 2 polyBLEP saws + SVF LP + amp ADSR.

use crate::harmony::Midi;

pub const MAX_VOICES: usize = 8;
pub const DEFAULT_SR: f32 = 44_100.0;

#[derive(Clone, Copy, Debug)]
pub struct Adsr {
    pub a: f32,
    pub d: f32,
    pub s: f32,
    pub r: f32,
}

impl Adsr {
    pub const fn amp_default() -> Self {
        // Warm pad-ish: slow attack, long release.
        Self {
            a: 0.05,
            d: 0.25,
            s: 0.7,
            r: 0.6,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Off,
    Attack,
    Decay,
    Sustain,
    Release,
}

#[derive(Clone, Copy, Debug)]
struct Env {
    stage: Stage,
    level: f32,
    adsr: Adsr,
}

impl Env {
    fn new(adsr: Adsr) -> Self {
        Self {
            stage: Stage::Off,
            level: 0.0,
            adsr,
        }
    }

    fn trigger(&mut self) {
        self.stage = Stage::Attack;
    }

    fn release(&mut self) {
        if self.stage != Stage::Off {
            self.stage = Stage::Release;
        }
    }

    fn active(&self) -> bool {
        self.stage != Stage::Off
    }

    fn process(&mut self, sr: f32) -> f32 {
        let a = self.adsr;
        match self.stage {
            Stage::Off => 0.0,
            Stage::Attack => {
                self.level += 1.0 / (a.a * sr).max(1.0);
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = Stage::Decay;
                }
                self.level
            }
            Stage::Decay => {
                self.level -= (1.0 - a.s) / (a.d * sr).max(1.0);
                if self.level <= a.s {
                    self.level = a.s;
                    self.stage = Stage::Sustain;
                }
                self.level
            }
            Stage::Sustain => a.s,
            Stage::Release => {
                self.level -= a.s.max(self.level) / (a.r * sr).max(1.0);
                if self.level <= 0.001 {
                    self.level = 0.0;
                    self.stage = Stage::Off;
                }
                self.level
            }
        }
    }
}

/// polyBLEP residual for a saw discontinuity at phase 0.
#[inline]
fn poly_blep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let x = t / dt;
        x + x - x * x - 1.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + x + x + 1.0
    } else {
        0.0
    }
}

#[derive(Clone, Copy, Debug)]
struct Osc {
    phase: f32,
    detune_cents: f32,
}

impl Osc {
    fn tick(&mut self, freq: f32, sr: f32) -> f32 {
        let f = freq * (2.0f32).powf(self.detune_cents / 1200.0);
        let dt = (f / sr).clamp(0.0, 0.49);
        let mut t = self.phase;
        let mut saw = 2.0 * t - 1.0;
        saw -= poly_blep(t, dt);
        t += dt;
        if t >= 1.0 {
            t -= 1.0;
        }
        self.phase = t;
        saw
    }
}

/// One-pole-ish state-variable filter (Chamberlin), LP output.
#[derive(Clone, Copy, Debug, Default)]
struct Svf {
    lp: f32,
    bp: f32,
}

impl Svf {
    fn process(&mut self, x: f32, cutoff_hz: f32, reso: f32, sr: f32) -> f32 {
        let f = 2.0 * (core::f32::consts::PI * (cutoff_hz / sr).clamp(0.0, 0.45)).sin();
        let q = (1.0 - reso.clamp(0.0, 0.95)).max(0.05);
        self.lp += f * self.bp;
        let hp = x - self.lp - q * self.bp;
        self.bp += f * hp;
        self.lp
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Voice {
    osc1: Osc,
    osc2: Osc,
    filt: Svf,
    amp: Env,
    midi: Midi,
    vel: f32,
    cutoff: f32,
    reso: f32,
    age: u32,
}

impl Voice {
    fn new() -> Self {
        Self {
            osc1: Osc {
                phase: 0.0,
                detune_cents: -7.0,
            },
            osc2: Osc {
                phase: 0.0,
                detune_cents: 7.0,
            },
            filt: Svf::default(),
            amp: Env::new(Adsr::amp_default()),
            midi: 0,
            vel: 0.0,
            cutoff: 2400.0,
            reso: 0.25,
            age: 0,
        }
    }

    pub fn active(&self) -> bool {
        self.amp.active()
    }

    fn note_on(&mut self, midi: Midi, vel: f32) {
        self.midi = midi;
        self.vel = vel.clamp(0.0, 1.0);
        self.age = 0;
        self.amp.trigger();
    }

    fn note_off(&mut self) {
        self.amp.release();
    }

    fn render(&mut self, sr: f32) -> f32 {
        if !self.active() {
            return 0.0;
        }
        self.age = self.age.saturating_add(1);
        let freq = 440.0 * (2.0f32).powf((self.midi as f32 - 69.0) / 12.0);
        let s = 0.5 * (self.osc1.tick(freq, sr) + self.osc2.tick(freq, sr));
        // mild key tracking on cutoff
        let cut = self.cutoff * (2.0f32).powf((self.midi as f32 - 60.0) / 24.0);
        let f = self.filt.process(s, cut, self.reso, sr);
        let g = self.amp.process(sr) * self.vel * 0.22;
        f * g
    }
}

/// Fixed pool of synth voices with simple steal-oldest allocation.
pub struct Engine {
    pub voices: [Voice; MAX_VOICES],
    pub sr: f32,
    /// Soft gain before i16 conversion.
    pub master: f32,
}

impl Engine {
    pub fn new(sr: f32) -> Self {
        Self {
            voices: [Voice::new(); MAX_VOICES],
            sr,
            master: 1.0,
        }
    }

    pub fn note_on(&mut self, midi: Midi, vel: f32) -> usize {
        // Reuse same midi if already sounding (re-trigger).
        if let Some(i) = self.voices.iter().position(|v| v.active() && v.midi == midi) {
            self.voices[i].note_on(midi, vel);
            return i;
        }
        if let Some(i) = self.voices.iter().position(|v| !v.active()) {
            self.voices[i].note_on(midi, vel);
            return i;
        }
        // Steal oldest.
        let i = self
            .voices
            .iter()
            .enumerate()
            .max_by_key(|(_, v)| v.age)
            .map(|(i, _)| i)
            .unwrap_or(0);
        self.voices[i].note_on(midi, vel);
        i
    }

    pub fn note_off(&mut self, midi: Midi) {
        for v in self.voices.iter_mut() {
            if v.active() && v.midi == midi {
                v.note_off();
            }
        }
    }

    pub fn all_notes_off(&mut self) {
        for v in self.voices.iter_mut() {
            v.note_off();
        }
    }

    pub fn active_count(&self) -> usize {
        self.voices.iter().filter(|v| v.active()).count()
    }

    /// Render `n_frames` stereo interleaved i16 samples into `out` (len >= n_frames*2).
    pub fn render_i16(&mut self, out: &mut [i16], n_frames: usize) {
        for i in 0..n_frames {
            let mut mix = 0.0f32;
            for v in self.voices.iter_mut() {
                mix += v.render(self.sr);
            }
            mix *= self.master;
            // soft clip
            mix = mix.tanh();
            let s = (mix * 30000.0).clamp(-32768.0, 32767.0) as i16;
            out[i * 2] = s;
            out[i * 2 + 1] = s;
        }
    }

    /// Render mono f32 samples (for bench).
    pub fn render_f32_mono(&mut self, out: &mut [f32]) {
        for s in out.iter_mut() {
            let mut mix = 0.0f32;
            for v in self.voices.iter_mut() {
                mix += v.render(self.sr);
            }
            *s = mix * self.master;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_alloc_and_release() {
        let mut e = Engine::new(44_100.0);
        e.note_on(60, 0.8);
        e.note_on(64, 0.8);
        e.note_on(67, 0.8);
        assert_eq!(e.active_count(), 3);
        e.note_off(64);
        // still active during release
        assert!(e.active_count() >= 2);
        for _ in 0..50_000 {
            let mut buf = [0.0f32; 64];
            e.render_f32_mono(&mut buf);
        }
        // released voice should go quiet eventually
        assert!(e.active_count() <= 2);
    }

    #[test]
    fn steal_when_full() {
        let mut e = Engine::new(44_100.0);
        for i in 0..MAX_VOICES + 2 {
            e.note_on(40 + i as u8, 0.5);
        }
        assert_eq!(e.active_count(), MAX_VOICES);
    }

    #[test]
    fn renders_nonzero() {
        let mut e = Engine::new(44_100.0);
        e.note_on(60, 1.0);
        // skip attack a bit
        let mut scratch = [0.0f32; 2048];
        e.render_f32_mono(&mut scratch);
        let mut buf = [0i16; 256];
        e.render_i16(&mut buf, 128);
        assert!(buf.iter().any(|&s| s != 0));
    }
}
