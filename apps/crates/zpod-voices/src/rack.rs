//! Voice rack: one slot per sequencer track, voices from a [`Bank`].

use crate::def::{Bank, Kind, VoiceDef, FIELDS};
use crate::dsp::*;
use zpod_seq::pattern::{Macros, Pattern, ECHO_TIMES, FX_MAX, TRACKS};
use zpod_seq::{Event, EventKind};

/// Slot index used for browser auditions (after the pattern tracks).
pub const AUDITION: usize = TRACKS;
const SLOTS: usize = TRACKS + 1;
/// Longest echo (2 s at 44.1 kHz covers 3/8 note at the slowest tempo).
pub const ECHO_MAX_S: f32 = 2.0;
const ECHO_FB: f32 = 0.42;

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
    snap: f32,
    snap_k: f32,
}

/// Clamp a field into the range the bank validator allows.
fn fclamp(name: &str, v: f32) -> f32 {
    match FIELDS.iter().find(|f| f.0 == name) {
        Some(&(_, _, lo, hi)) => v.clamp(lo, hi),
        None => v,
    }
}

/// A voice definition with a track's macros applied (done at note-on,
/// outside the per-sample loop). Pitch is applied by the sequencer.
pub fn shape(d: VoiceDef, m: Macros) -> VoiceDef {
    if m == Macros::default() {
        return d;
    }
    let mut d = d;
    let bright = (m.tone as f32 / 6.0).exp2(); // ×0.31 .. ×3.2
    let len = (m.decay as f32 / 6.0).exp2();
    d.cutoff = fclamp("cutoff", d.cutoff * bright);
    d.tone = fclamp("tone", d.tone + 0.04 * m.tone as f32);
    d.decay = fclamp("decay", d.decay * len);
    d.release = fclamp("release", d.release * len);
    d.drive = fclamp("drive", d.drive + 0.08 * m.drive as f32);
    d
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
            snap: 0.0,
            snap_k: 0.0,
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
        let v = self.vel;
        // velocity changes timbre and length, not only level: soft hits are
        // darker and shorter, hard hits brighter with more transient
        let vb = 0.7 + 0.5 * v; // brightness factor 0.7..1.2
        let vl = 0.8 + 0.25 * v; // length factor 0.8..1.05
        match d.kind {
            Kind::Kick | Kind::Snare | Kind::Hat => {
                self.hz = d.pitch * tune;
                self.ph = [0.0; 6];
                self.amp = v;
                self.penv = 0.55 + 0.45 * v; // sweep depth follows velocity
                self.penv_k = decay_k(d.sweep_time, sr);
                self.aux = v;
                match d.kind {
                    Kind::Kick => {
                        self.amp_k = decay_k(d.decay * vl, sr);
                        self.aux_k = decay_k(0.0035, sr);
                        // transient layer: band-passed noise click
                        self.f1.set((d.cutoff * 1.6 * vb).min(12_000.0), 0.35, sr);
                    }
                    Kind::Snare => {
                        self.amp_k = decay_k(d.decay * (0.3 + 0.6 * d.tone) * vl, sr);
                        self.aux_k = decay_k(d.decay * vl, sr);
                        self.f1.set((d.cutoff * (0.55 + 0.7 * v)).min(14_000.0), d.reso, sr);
                        // transient layer: short bright snap (high-passed noise)
                        self.snap = v * v;
                        self.snap_k = decay_k(0.007, sr);
                        self.f2.set((d.cutoff * 2.2).min(15_000.0), 0.0, sr);
                    }
                    _ => {
                        self.amp_k = decay_k(d.decay * vl, sr);
                        self.f1.set((d.cutoff * vb).min(15_000.0), 0.1, sr);
                        self.f2.set((d.cutoff * 2.0 * vb).min(16_000.0), 0.0, sr);
                        for (i, p) in self.ph.iter_mut().enumerate() {
                            *p = (i as f32 * 0.137) % 1.0;
                        }
                    }
                }
            }
            Kind::Clap => {
                self.amp = 0.0;
                self.amp_k = decay_k(d.decay * vl, sr);
                self.aux = v;
                self.aux_k = decay_k(0.009, sr);
                self.f1.set((d.cutoff * vb).min(12_000.0), d.reso, sr);
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
        self.snap_k = self.snap_k.min(k);
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
                    let (_, bp, _) = self.f1.run(self.noise.next());
                    let click = bp * 1.8 * self.aux * d.tone;
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
                    let (_, _, sh) = self.f2.run(self.noise.next());
                    let snap = sh * self.snap * 0.9;
                    self.amp *= self.amp_k;
                    self.aux *= self.aux_k;
                    self.snap *= self.snap_k;
                    let nmix = (d.noise * (0.75 + 0.35 * self.vel)).min(1.0);
                    let s = sat(body * (1.0 - nmix) * 1.3 + nz * nmix * 1.5 + snap) * lvl;
                    *o += self.post(s);
                    if self.amp < 1e-4 && self.aux < 1e-4 && self.snap < 1e-4 { self.active = false; break; }
                }
            }
            Kind::Hat => {
                for o in out.iter_mut() {
                    let mut metal = 0.0;
                    for (i, r) in HAT_RATIOS.iter().enumerate() {
                        let dt = self.hz * r * inv;
                        self.ph[i] = wrap(self.ph[i] + dt);
                        metal += square(self.ph[i], dt); // polyBLEP: no naive aliasing
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
        // Fast click/snap/sweep envelopes decay into subnormal floats while
        // the body still rings; ARM11 VFP handles those ~100x slower (a long
        // kick measured 49 % of a core). Zero them once inaudible.
        for v in [&mut self.aux, &mut self.snap, &mut self.penv] {
            if v.abs() < 1e-9 {
                *v = 0.0;
            }
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
    pub macros: Macros,
    /// echo send 0..1
    pub send: f32,
}

/// Default stereo placement per track (kick, snare, hat, bass, lead, audition).
const PANS: [f32; SLOTS] = [0.0, 0.08, 0.3, 0.0, -0.25, 0.0];

pub struct Rack {
    pub bank: Bank,
    pub slots: [Slot; SLOTS],
    scratch: Box<[f32]>,
    /// mono echo send bus for the current block
    fx_in: Box<[f32]>,
    echo: Box<[f32]>,
    echo_w: usize,
    echo_len: usize,
    echo_want: usize,
    echo_mix: f32,
    echo_cur: f32,
    echo_lp: (f32, f32),
    echo_clear: usize,
    /// master drive 0..1
    pub drive: f32,
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
                macros: Macros::default(),
                send: 0.0,
            }
        });
        let echo_n = (ECHO_MAX_S * sr) as usize;
        Rack {
            bank,
            slots,
            scratch: vec![0.0; MAX_BLOCK].into_boxed_slice(),
            fx_in: vec![0.0; MAX_BLOCK].into_boxed_slice(),
            echo: vec![0.0; echo_n * 2].into_boxed_slice(),
            echo_w: 0,
            echo_len: echo_n / 4,
            echo_want: echo_n / 4,
            echo_mix: 0.0,
            echo_cur: 0.0,
            echo_lp: (0.0, 0.0),
            echo_clear: usize::MAX,
            drive: 0.0,
            dc: [(0.0, 0.0); 2],
            lim: 0.0,
            lim_rel: decay_k(0.15, sr),
            out_gain: 0.5,
            out_cur: 0.5,
            peak: 0.0,
            overs: 0,
        }
    }

    /// Take everything the rack needs from a pattern: voices, levels,
    /// mute/solo, macros, sends, drive and echo (time follows the tempo).
    /// Returns a bit per track whose voice id is not in the bank (that
    /// track keeps its previous voice). No allocation.
    pub fn apply_pattern(&mut self, p: &Pattern, sample_rate: u32) -> u8 {
        let mut unknown = 0u8;
        for (t, tr) in p.tracks.iter().enumerate() {
            if !self.set_track(t, tr.voice.as_str(), tr.level as f32 / 127.0, !p.audible(t)) {
                unknown |= 1 << t;
            }
            self.slots[t].macros = tr.macros;
            self.slots[t].send = tr.send.min(FX_MAX) as f32 / FX_MAX as f32;
        }
        self.drive = p.drive.min(FX_MAX) as f32 / FX_MAX as f32;
        let beats = ECHO_TIMES[(p.echo_time as usize).min(ECHO_TIMES.len() - 1)].1;
        self.set_echo(p.echo.min(FX_MAX) as f32 / FX_MAX as f32 * 0.6, (beats * 60.0 / p.tempo as f64 * sample_rate as f64) as usize);
        unknown
    }

    /// Echo return level (0..1) and delay in frames (clamped to the buffer).
    /// A time change fades the echo out, moves, and fades back in.
    pub fn set_echo(&mut self, mix: f32, frames: usize) {
        let mix = mix.clamp(0.0, 1.0);
        if self.echo_mix == 0.0 && mix > 0.0 && self.echo_cur < 1e-5 {
            self.echo_len = 0; // stale buffer from earlier: wipe before use
        }
        self.echo_mix = mix;
        self.echo_want = frames.clamp(64, self.echo.len() / 2 - 1);
    }

    /// Prepare the audition slot with a bank voice and the given macros.
    pub fn set_audition(&mut self, voice_index: usize, macros: Macros) -> bool {
        let Some(&def) = self.bank.voices.get(voice_index) else { return false };
        let s = &mut self.slots[AUDITION];
        s.def = def;
        s.level = 0.9;
        s.mute = false;
        s.macros = macros;
        s.send = 0.0;
        true
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
                let def = shape(s.def, s.macros);
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
        self.fx_in[..frames].iter_mut().for_each(|x| *x = 0.0);
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
            let send = s.send;
            for (i, x) in buf.iter().enumerate() {
                s.cur += (target - s.cur) * 0.003;
                let y = x * s.cur;
                out[2 * (a + i)] += y * s.pan.0;
                out[2 * (a + i) + 1] += y * s.pan.1;
                self.fx_in[a + i] += y * send;
            }
        }
    }

    fn master(&mut self, out: &mut [f32]) {
        let n2 = self.echo.len() / 2;
        let dg = 1.0 + 3.0 * self.drive;
        let dn = 1.0 / (1.0 + 1.2 * self.drive);
        let wet_on = self.echo_mix > 0.0 || self.echo_cur > 1e-5;
        for (i, fr) in out.chunks_exact_mut(2).enumerate() {
            if wet_on {
                // ping-pong echo with damped feedback; time changes fade
                let want = if self.echo_want != self.echo_len { 0.0 } else { self.echo_mix };
                self.echo_cur += (want - self.echo_cur) * 0.004;
                if self.echo_want != self.echo_len && self.echo_cur < 1e-3 {
                    // wipe the old echo a few frames per sample (no CPU spike)
                    if self.echo_clear >= n2 {
                        self.echo_clear = 0;
                    }
                    let c = self.echo_clear;
                    let e = (c + 3).min(n2);
                    self.echo[2 * c..2 * e].iter_mut().for_each(|x| *x = 0.0);
                    self.echo_clear = e;
                    if e >= n2 {
                        self.echo_len = self.echo_want;
                        self.echo_lp = (0.0, 0.0);
                    }
                }
                let r = (self.echo_w + n2 - self.echo_len) % n2;
                let (dl, dr) = (self.echo[2 * r], self.echo[2 * r + 1]);
                self.echo_lp.0 += (dl - self.echo_lp.0) * 0.45;
                self.echo_lp.1 += (dr - self.echo_lp.1) * 0.45;
                let w = self.echo_w;
                // (tiny values flushed: a decayed tail must not go subnormal)
                let l = self.fx_in[i] + ECHO_FB * self.echo_lp.1;
                let r = ECHO_FB * self.echo_lp.0;
                self.echo[2 * w] = if l.abs() < 1e-12 { 0.0 } else { l };
                self.echo[2 * w + 1] = if r.abs() < 1e-12 { 0.0 } else { r };
                self.echo_w = if w + 1 >= n2 { 0 } else { w + 1 };
                fr[0] += dl * self.echo_cur;
                fr[1] += dr * self.echo_cur;
            }
            for (c, x) in fr.iter_mut().enumerate() {
                let (x1, y1) = self.dc[c];
                let y = *x - x1 + 0.9995 * y1;
                self.dc[c] = (*x, y);
                *x = if self.drive > 0.0 { sat(y * BUS * dg) * dn } else { y * BUS };
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
        // decaying master state (DC blocker, echo damping, limiter) also
        // stays out of the subnormal range during silence
        let tiny = |v: &mut f32| {
            if v.abs() < 1e-12 {
                *v = 0.0;
            }
        };
        for d in self.dc.iter_mut() {
            tiny(&mut d.0);
            tiny(&mut d.1);
        }
        tiny(&mut self.echo_lp.0);
        tiny(&mut self.echo_lp.1);
        tiny(&mut self.lim);
    }
}
