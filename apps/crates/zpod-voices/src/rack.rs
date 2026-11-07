//! Voice rack: one slot per sequencer track, voices from a [`Bank`].

use crate::def::{Bank, Kind, VoiceDef};
use crate::dsp::*;
use zpod_seq::pattern::TRACKS;
use zpod_seq::{Event, EventKind};

pub const MAX_BLOCK: usize = 1024;
pub const MAX_POLY: usize = 4;
pub const KS_LEN: usize = 2048;
/// Limiter ceiling (≈ -1 dBFS).
pub const CEIL: f32 = 0.89;
const C4: f32 = 261.625_6;
const HAT_RATIOS: [f32; 6] = [1.0, 1.4471, 1.617, 1.9265, 2.5028, 2.6637];
/// Mix bus gain before the limiter (headroom for 5 tracks).
const BUS: f32 = 0.55;

pub struct Voice {
    pub id: u32,
    pub gate: bool,
    active: bool,
    age: u32,
    def: VoiceDef,
    sr: f32,
    hz: f32,
    target: f32,
    glide_k: f32,
    vel: f32,
    ph: [f32; 6],
    amp: f32,
    amp_k: f32,
    penv: f32,
    penv_k: f32,
    aux: f32,
    aux_k: f32,
    env: Adsr,
    f1: Svf,
    f2: Svf,
    noise: Noise,
    t: u32,
    ks: Box<[f32]>,
    ks_w: usize,
    ks_len: f32,
    ks_fb: f32,
    ks_rel_fb: f32,
    ks_prev: f32,
    last: f32,
    click: f32,
    ctr: u32,
}

impl Voice {
    fn new(def: VoiceDef, sr: f32, seed: u32) -> Self {
        Voice {
            id: 0,
            gate: false,
            active: false,
            age: 0,
            def,
            sr,
            hz: 100.0,
            target: 100.0,
            glide_k: 0.0,
            vel: 0.0,
            ph: [0.0; 6],
            amp: 0.0,
            amp_k: 0.0,
            penv: 0.0,
            penv_k: 0.0,
            aux: 0.0,
            aux_k: 0.0,
            env: Adsr::default(),
            f1: Svf::default(),
            f2: Svf::default(),
            noise: Noise(seed | 1),
            t: 0,
            ks: vec![0.0; KS_LEN].into_boxed_slice(),
            ks_w: 0,
            ks_len: 100.0,
            ks_fb: 0.0,
            ks_rel_fb: 0.0,
            ks_prev: 0.0,
            last: 0.0,
            click: 0.0,
            ctr: 0,
        }
    }

    pub fn active(&self) -> bool {
        self.active
    }

    fn trigger(&mut self, def: VoiceDef, hz: f32, vel: f32, id: u32, legato: bool) {
        let sr = self.sr;
        if self.active && !legato {
            self.click += self.last; // continue from where the old sound was
        } else if !self.active {
            self.f1.reset();
            self.f2.reset();
            self.click = 0.0;
        }
        self.def = def;
        // noise depends on the note id, not on which voice instance plays it
        self.noise = Noise(id.wrapping_mul(0x9E37_79B9) ^ 0x5bd1_e995 | 1);
        self.vel = vel.clamp(0.0, 1.0);
        self.id = id;
        self.gate = true;
        self.age = 0;
        self.t = 0;
        let d = &self.def;
        let tune = hz / C4;
        match d.kind {
            Kind::Kick | Kind::Snare | Kind::Hat => {
                self.hz = d.pitch * tune;
                self.ph = [0.0; 6];
                self.amp = self.vel;
                self.penv = 1.0;
                self.penv_k = decay_k(d.sweep_time, sr);
                self.aux = self.vel;
                match d.kind {
                    Kind::Kick => {
                        self.amp_k = decay_k(d.decay, sr);
                        self.aux_k = decay_k(0.004, sr);
                    }
                    Kind::Snare => {
                        self.amp_k = decay_k(d.decay * (0.3 + 0.6 * d.tone), sr);
                        self.aux_k = decay_k(d.decay, sr);
                        self.f1.set(d.cutoff, d.reso, sr);
                    }
                    _ => {
                        self.amp_k = decay_k(d.decay, sr);
                        self.f1.set(d.cutoff, 0.1, sr);
                        self.f2.set((d.cutoff * 2.0).min(16_000.0), 0.0, sr);
                        for (i, p) in self.ph.iter_mut().enumerate() {
                            *p = (i as f32 * 0.137) % 1.0;
                        }
                    }
                }
            }
            Kind::Clap => {
                self.amp = 0.0;
                self.amp_k = decay_k(d.decay, sr);
                self.aux = self.vel;
                self.aux_k = decay_k(0.009, sr);
                self.f1.set(d.cutoff, d.reso, sr);
            }
            Kind::Mono | Kind::Poly | Kind::Fm => {
                let glide = legato && d.glide > 0.0 && self.active;
                self.target = hz;
                if !glide {
                    self.hz = hz;
                    if !self.active {
                        self.ph = [0.0; 6];
                    }
                }
                self.glide_k = if d.glide > 0.0 { 1.0 - decay_k(d.glide, sr) } else { 0.0 };
                self.env.set(d.attack, d.decay, d.sustain, d.release, sr);
                self.env.gate_on();
                self.aux = 1.0;
                self.aux_k = decay_k(if d.kind == Kind::Fm { d.decay * 0.6 } else { d.decay * 1.5 }, sr);
                self.ctr = 0;
            }
            Kind::Pluck => {
                self.hz = hz;
                self.ks_len = (sr / hz - 0.5).clamp(2.0, (KS_LEN - 3) as f32);
                let b = 0.12 + 0.85 * d.tone;
                let mut lp = 0.0;
                let mut mean = 0.0;
                for s in self.ks.iter_mut() {
                    lp += (self.noise.next() - lp) * b;
                    *s = lp;
                    mean += lp;
                }
                mean /= KS_LEN as f32;
                for s in self.ks.iter_mut() {
                    *s = (*s - mean) * self.vel * 1.6;
                }
                self.ks_prev = 0.0;
                self.ks_fb = (0.001f32.ln() / (hz * d.decay)).exp();
                self.ks_rel_fb = (0.001f32.ln() / (hz * d.release)).exp();
                self.env.set(0.0015, 8.0, 1.0, d.release, sr);
                self.env.level = 0.0;
                self.env.gate_on();
                self.f1.set(d.cutoff, 0.0, sr);
            }
        }
        self.active = true;
    }

    fn release(&mut self) {
        self.gate = false;
        match self.def.kind {
            Kind::Mono | Kind::Poly | Kind::Fm => self.env.gate_off(),
            Kind::Pluck => {
                self.env.gate_off();
                self.ks_fb = self.ks_fb.min(self.ks_rel_fb);
            }
            _ => {} // drums are one-shots
        }
    }

    /// Fast fade (hat choke, steal) in ~4 ms.
    fn choke(&mut self) {
        self.gate = false;
        let k = decay_k(0.004, self.sr);
        self.amp_k = self.amp_k.min(k);
        self.aux_k = self.aux_k.min(k);
    }

    /// Add this voice into `out` (mono).
    fn render_add(&mut self, out: &mut [f32]) {
        let sr = self.sr;
        let inv = 1.0 / sr;
        let d = self.def;
        let lvl = d.level;
        match d.kind {
            Kind::Kick => {
                let g = 1.0 + 3.0 * d.drive;
                for o in out.iter_mut() {
                    let f = self.hz * (1.0 + (d.sweep - 1.0) * self.penv);
                    self.penv *= self.penv_k;
                    self.ph[0] = wrap(self.ph[0] + f * inv);
                    let body = sin_turns(self.ph[0]) * self.amp;
                    let click = self.noise.next() * self.aux * d.tone;
                    self.aux *= self.aux_k;
                    self.amp *= self.amp_k;
                    let s = sat((body + click) * g) * lvl;
                    *o += self.post(s);
                    if self.amp < 1e-4 { self.active = false; break; }
                }
            }
            Kind::Snare => {
                for o in out.iter_mut() {
                    let f = self.hz * (1.0 + (d.sweep - 1.0) * self.penv);
                    self.penv *= self.penv_k;
                    self.ph[0] = wrap(self.ph[0] + f * inv);
                    self.ph[1] = wrap(self.ph[1] + f * 1.47 * inv);
                    let body = (0.6 * sin_turns(self.ph[0]) + 0.4 * sin_turns(self.ph[1])) * self.amp;
                    let (_, bp, hp) = self.f1.run(self.noise.next());
                    let nz = (0.6 * hp + 0.8 * bp) * self.aux;
                    self.amp *= self.amp_k;
                    self.aux *= self.aux_k;
                    let s = sat(body * (1.0 - d.noise) * 1.3 + nz * d.noise * 1.5) * lvl;
                    *o += self.post(s);
                    if self.amp < 1e-4 && self.aux < 1e-4 { self.active = false; break; }
                }
            }
            Kind::Hat => {
                for o in out.iter_mut() {
                    let mut metal = 0.0;
                    for (i, r) in HAT_RATIOS.iter().enumerate() {
                        self.ph[i] = wrap(self.ph[i] + self.hz * r * inv);
                        metal += if self.ph[i] < 0.5 { 1.0 } else { -1.0 };
                    }
                    let x = metal * (1.0 / 6.0) * d.tone + self.noise.next() * (1.0 - d.tone);
                    let (_, _, hp) = self.f1.run(x);
                    let (lp, _, _) = self.f2.run(hp);
                    let s = lp * self.amp * 1.6 * lvl;
                    self.amp *= self.amp_k;
                    *o += self.post(s);
                    if self.amp < 1e-4 { self.active = false; break; }
                }
            }
            Kind::Clap => {
                let b = (0.011 * sr) as u32;
                for o in out.iter_mut() {
                    if self.t < 3 * b && self.t % b == 0 && self.t > 0 {
                        self.aux = self.vel * (1.0 - 0.15 * (self.t / b) as f32);
                    }
                    if self.t == 2 * b {
                        self.amp = self.vel * 0.75;
                    }
                    self.t = self.t.saturating_add(1);
                    let (_, bp, _) = self.f1.run(self.noise.next());
                    let e = self.aux + self.amp;
                    self.aux *= self.aux_k;
                    self.amp *= self.amp_k;
                    let s = sat(bp * e * 2.2) * lvl;
                    *o += self.post(s);
                    if self.t > 3 * b && self.amp < 1e-4 && self.aux < 1e-4 { self.active = false; break; }
                }
            }
            Kind::Mono | Kind::Poly => {
                let mono = d.kind == Kind::Mono;
                let det = (d.detune / 1200.0).exp2();
                let g = 1.0 + 2.0 * d.drive;
                for o in out.iter_mut() {
                    if self.ctr % 16 == 0 {
                        let fc = d.cutoff * (1.0 + d.env_amt * 8.0 * self.aux * (0.5 + 0.5 * self.vel));
                        self.f1.set(fc, d.reso, sr);
                        if mono {
                            self.f2.set(fc * 1.2, 0.0, sr);
                        }
                    }
                    self.ctr = self.ctr.wrapping_add(1);
                    self.aux *= self.aux_k;
                    if self.glide_k > 0.0 {
                        self.hz += (self.target - self.hz) * self.glide_k;
                    } else {
                        self.hz = self.target;
                    }
                    let dt = self.hz * inv;
                    self.ph[0] = wrap(self.ph[0] + dt);
                    let mut x = saw(self.ph[0], dt) * (1.0 - d.wave) + square(self.ph[0], dt) * d.wave;
                    if mono {
                        self.ph[1] = wrap(self.ph[1] + dt * 0.5);
                        x = x * 0.7 + square(self.ph[1], dt * 0.5) * d.tone * 0.6;
                    } else {
                        let dt2 = dt * det;
                        self.ph[1] = wrap(self.ph[1] + dt2);
                        x = 0.5 * x + 0.5 * (saw(self.ph[1], dt2) * (1.0 - d.wave) + square(self.ph[1], dt2) * d.wave);
                    }
                    let (mut y, _, _) = self.f1.run(x * 0.7);
                    if mono {
                        y = self.f2.run(y).0;
                    }
                    let e = self.env.next();
                    let s = sat(y * g) * e * self.vel * lvl;
                    *o += self.post(s);
                    if !self.env.active() { self.active = false; break; }
                }
            }
            Kind::Pluck => {
                for o in out.iter_mut() {
                    let mut r = self.ks_w as f32 - self.ks_len;
                    if r < 0.0 {
                        r += KS_LEN as f32;
                    }
                    let i0 = r as usize;
                    let fr = r - i0 as f32;
                    let i1 = if i0 + 1 >= KS_LEN { 0 } else { i0 + 1 };
                    let y = self.ks[i0] * (1.0 - fr) + self.ks[i1] * fr;
                    let v = self.ks_fb * 0.5 * (y + self.ks_prev);
                    self.ks_prev = y;
                    self.ks[self.ks_w] = v;
                    self.ks_w += 1;
                    if self.ks_w >= KS_LEN {
                        self.ks_w = 0;
                    }
                    let (lp, _, _) = self.f1.run(v);
                    let s = sat(lp) * self.env.next() * lvl;
                    *o += self.post(s);
                    if !self.env.active() { self.active = false; break; }
                }
            }
            Kind::Fm => {
                let inv_tau = 1.0 / core::f32::consts::TAU;
                for o in out.iter_mut() {
                    self.ph[1] = wrap(self.ph[1] + self.hz * d.ratio * inv);
                    let m = sin_turns(self.ph[1]) * d.index * self.aux * (0.4 + 0.6 * self.vel);
                    self.aux *= self.aux_k;
                    self.ph[0] = wrap(self.ph[0] + self.hz * inv);
                    let s = sin_turns(self.ph[0] + m * inv_tau) * self.env.next() * self.vel * lvl;
                    *o += self.post(s);
                    if !self.env.active() { self.active = false; break; }
                }
            }
        }
        if !self.active {
            self.last = 0.0;
        }
        self.age = self.age.saturating_add(out.len() as u32);
    }

    #[inline]
    fn post(&mut self, s: f32) -> f32 {
        let y = s + self.click;
        self.click *= 0.94;
        if self.click.abs() < 1e-6 {
            self.click = 0.0;
        }
        self.last = y;
        y
    }
}

pub struct Slot {
    pub def: VoiceDef,
    voices: [Voice; MAX_POLY],
    pub level: f32,
    cur: f32,
    pan: (f32, f32),
    pub mute: bool,
    mono_id: u32,
}

/// Default stereo placement per track (kick, snare, hat, bass, lead).
const PANS: [f32; TRACKS] = [0.0, 0.08, 0.3, 0.0, -0.25];

pub struct Rack {
    pub bank: Bank,
    pub slots: [Slot; TRACKS],
    scratch: Box<[f32]>,
    dc: [(f32, f32); 2],
    lim: f32,
    lim_rel: f32,
    /// output gain after the limiter (0..1), smoothed
    pub out_gain: f32,
    out_cur: f32,
    /// highest output sample since last read (meters / tests)
    pub peak: f32,
    pub overs: u64,
}

impl Rack {
    pub fn new(sample_rate: u32, bank: Bank) -> Self {
        let sr = sample_rate as f32;
        let def0 = bank.voices[0];
        let slots = core::array::from_fn(|t| {
            let p = PANS[t];
            let a = (p + 1.0) * core::f32::consts::FRAC_PI_4;
            Slot {
                def: def0,
                voices: core::array::from_fn(|v| Voice::new(def0, sr, 0x1234_5677 ^ ((t * 8 + v) as u32).wrapping_mul(0x9E37_79B9))),
                level: 0.8,
                cur: 0.8,
                pan: (a.cos() * 1.414, a.sin() * 1.414),
                mute: false,
                mono_id: 0,
            }
        });
        Rack { bank, slots, scratch: vec![0.0; MAX_BLOCK].into_boxed_slice(), dc: [(0.0, 0.0); 2], lim: 0.0, lim_rel: decay_k(0.15, sr), out_gain: 0.5, out_cur: 0.5, peak: 0.0, overs: 0 }
    }

    /// Point a track at a voice id (no allocation; unknown ids keep the old voice).
    pub fn set_track(&mut self, t: usize, voice: &str, level: f32, mute: bool) -> bool {
        let s = &mut self.slots[t];
        s.level = level.clamp(0.0, 1.0);
        s.mute = mute;
        if s.def.id.as_str() == voice {
            return true;
        }
        match self.bank.find(voice) {
            Some(i) => {
                s.def = self.bank.voices[i];
                true
            }
            None => false,
        }
    }

    pub fn active_voices(&self) -> usize {
        self.slots.iter().map(|s| s.voices.iter().filter(|v| v.active).count()).sum()
    }

    pub fn handle(&mut self, kind: &EventKind) {
        match *kind {
            EventKind::NoteOn { track, id, pitch, vel } => {
                let Some(s) = self.slots.get_mut(track as usize) else { return };
                if s.mute {
                    return;
                }
                let def = s.def;
                let poly = def.kind.polyphony().min(MAX_POLY);
                if def.kind == Kind::Mono {
                    let v = &mut s.voices[0];
                    let legato = v.active && v.gate;
                    v.trigger(def, pitch.hz, vel, id, legato);
                    s.mono_id = id;
                    for v in s.voices[1..].iter_mut() {
                        if v.active {
                            v.choke();
                        }
                    }
                    return;
                }
                let i = (0..poly).find(|&i| !s.voices[i].active).unwrap_or_else(|| (0..poly).max_by_key(|&i| s.voices[i].age).unwrap_or(0));
                if def.kind == Kind::Hat {
                    for (j, v) in s.voices.iter_mut().enumerate() {
                        if j != i && v.active {
                            v.choke();
                        }
                    }
                }
                s.voices[i].trigger(def, pitch.hz, vel, id, false);
            }
            EventKind::NoteOff { track, id } => {
                let Some(s) = self.slots.get_mut(track as usize) else { return };
                for v in s.voices.iter_mut() {
                    if v.active && v.gate && v.id == id {
                        v.release();
                    }
                }
            }
            EventKind::AllNotesOff => {
                for s in self.slots.iter_mut() {
                    for v in s.voices.iter_mut() {
                        if v.active {
                            v.release();
                        }
                    }
                }
            }
        }
    }

    /// Render `frames` stereo frames into `out` (interleaved, len ≥ 2·frames),
    /// applying each event at its offset. Overwrites `out`.
    pub fn render(&mut self, events: &[Event], out: &mut [f32], frames: usize) {
        let frames = frames.min(MAX_BLOCK).min(out.len() / 2);
        out[..frames * 2].iter_mut().for_each(|x| *x = 0.0);
        let mut a = 0usize;
        for e in events {
            let off = (e.offset as usize).min(frames);
            if off > a {
                self.segment(out, a, off);
                a = off;
            }
            self.handle(&e.kind);
        }
        if a < frames {
            self.segment(out, a, frames);
        }
        self.master(&mut out[..frames * 2]);
    }

    fn segment(&mut self, out: &mut [f32], a: usize, b: usize) {
        let n = b - a;
        for s in self.slots.iter_mut() {
            let any = s.voices.iter().any(|v| v.active);
            let target = if s.mute { 0.0 } else { s.level };
            if !any {
                s.cur = target;
                continue;
            }
            let buf = &mut self.scratch[..n];
            buf.iter_mut().for_each(|x| *x = 0.0);
            for v in s.voices.iter_mut() {
                if v.active {
                    v.render_add(buf);
                }
            }
            for (i, x) in buf.iter().enumerate() {
                s.cur += (target - s.cur) * 0.003;
                let y = x * s.cur;
                out[2 * (a + i)] += y * s.pan.0;
                out[2 * (a + i) + 1] += y * s.pan.1;
            }
        }
    }

    fn master(&mut self, out: &mut [f32]) {
        for fr in out.chunks_exact_mut(2) {
            for (c, x) in fr.iter_mut().enumerate() {
                let (x1, y1) = self.dc[c];
                let y = *x - x1 + 0.9995 * y1;
                self.dc[c] = (*x, y);
                *x = y * BUS;
            }
            let pk = fr[0].abs().max(fr[1].abs());
            self.lim = if pk > self.lim { pk } else { self.lim * self.lim_rel + pk * (1.0 - self.lim_rel) };
            let g = if self.lim > CEIL { CEIL / self.lim } else { 1.0 };
            self.out_cur += (self.out_gain.clamp(0.0, 1.0) - self.out_cur) * 0.002;
            for x in fr.iter_mut() {
                let y = *x * g;
                if y.abs() > 0.999 {
                    self.overs += 1;
                }
                *x = y.clamp(-1.0, 1.0) * self.out_cur;
                self.peak = self.peak.max(x.abs());
            }
        }
    }
}
