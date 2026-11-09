//! DSP primitives (f32, cheap on ARMv6 VFP).

use core::f32::consts::PI;

/// sin(2π·p) for p in turns (any real), ~0.1 % error, no libm.
#[inline]
pub fn sin_turns(p: f32) -> f32 {
    let x = p - (p + 0.5).floor(); // -0.5..0.5
    let y = 8.0 * x - 16.0 * x * x.abs();
    0.225 * (y * y.abs() - y) + y
}

/// Smooth saturation, ≈tanh for |x| < 3, bounded ±1.
#[inline]
pub fn sat(x: f32) -> f32 {
    let x = x.clamp(-3.0, 3.0);
    x * (27.0 + x * x) / (27.0 + 9.0 * x * x)
}

#[inline]
pub fn blep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let t = t / dt;
        t + t - t * t - 1.0
    } else if t > 1.0 - dt {
        let t = (t - 1.0) / dt;
        t * t + t + t + 1.0
    } else {
        0.0
    }
}

/// Band-limited saw from phase (0..1) and phase increment.
#[inline]
pub fn saw(ph: f32, dt: f32) -> f32 {
    2.0 * ph - 1.0 - blep(ph, dt)
}

/// Band-limited 50 % square.
#[inline]
pub fn square(ph: f32, dt: f32) -> f32 {
    let mut s = if ph < 0.5 { 1.0 } else { -1.0 };
    s += blep(ph, dt);
    let p2 = if ph >= 0.5 { ph - 0.5 } else { ph + 0.5 };
    s - blep(p2, dt)
}

#[inline]
pub fn wrap(p: f32) -> f32 {
    if p >= 1.0 { p - 1.0 } else { p }
}

/// Per-sample multiplier that decays to -60 dB in `t` seconds.
pub fn decay_k(t: f32, sr: f32) -> f32 {
    (-6.9078 / (t.max(0.0005) * sr)).exp()
}

pub struct Noise(pub u32);
impl Noise {
    #[inline]
    pub fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x as i32 as f32) * (1.0 / 2_147_483_648.0)
    }
}

/// Topology-preserving state-variable filter (Simper/Zavalishin).
#[derive(Clone, Copy, Default, Debug)]
pub struct Svf {
    ic1: f32,
    ic2: f32,
    a1: f32,
    a2: f32,
    a3: f32,
    k: f32,
}

impl Svf {
    pub fn set(&mut self, fc: f32, reso: f32, sr: f32) {
        let g = (PI * fc.clamp(20.0, sr * 0.45) / sr).tan();
        self.k = 2.0 - 2.0 * reso.clamp(0.0, 0.97);
        self.a1 = 1.0 / (1.0 + g * (g + self.k));
        self.a2 = g * self.a1;
        self.a3 = g * self.a2;
    }
    pub fn reset(&mut self) {
        self.ic1 = 0.0;
        self.ic2 = 0.0;
    }
    /// (low, band, high)
    #[inline]
    pub fn run(&mut self, v0: f32) -> (f32, f32, f32) {
        let v3 = v0 - self.ic2;
        let v1 = self.a1 * self.ic1 + self.a2 * v3;
        let v2 = self.ic2 + self.a2 * self.ic1 + self.a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        // flush denormals / runaway
        if !(self.ic1.abs() < 1e3) {
            self.ic1 = 0.0;
        }
        if !(self.ic2.abs() < 1e3) {
            self.ic2 = 0.0;
        }
        (v2, v1, v0 - self.k * v1 - v2)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    Off,
    Attack,
    Decay,
    Release,
}

/// Linear attack, exponential decay to sustain, exponential release.
#[derive(Clone, Copy, Debug)]
pub struct Adsr {
    pub stage: Stage,
    pub level: f32,
    a_inc: f32,
    d_k: f32,
    s: f32,
    r_k: f32,
}

impl Default for Adsr {
    fn default() -> Self {
        Adsr { stage: Stage::Off, level: 0.0, a_inc: 1.0, d_k: 0.99, s: 0.0, r_k: 0.99 }
    }
}

impl Adsr {
    pub fn set(&mut self, a: f32, d: f32, s: f32, r: f32, sr: f32) {
        self.a_inc = 1.0 / (a * sr).max(16.0); // ≥ 16 samples: no click
        self.d_k = decay_k(d, sr);
        self.s = s;
        self.r_k = decay_k(r, sr);
    }
    /// Retrigger from the current level (no jump).
    pub fn gate_on(&mut self) {
        self.stage = Stage::Attack;
    }
    pub fn gate_off(&mut self) {
        if self.stage != Stage::Off {
            self.stage = Stage::Release;
        }
    }
    #[inline]
    pub fn next(&mut self) -> f32 {
        match self.stage {
            Stage::Off => {}
            Stage::Attack => {
                self.level += self.a_inc;
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = Stage::Decay;
                }
            }
            Stage::Decay => {
                self.level = self.s + (self.level - self.s) * self.d_k;
                if self.s < 1e-4 && self.level < 1e-4 {
                    self.stage = Stage::Off;
                    self.level = 0.0;
                }
            }
            Stage::Release => {
                self.level *= self.r_k;
                if self.level < 1e-4 {
                    self.stage = Stage::Off;
                    self.level = 0.0;
                }
            }
        }
        self.level
    }
    pub fn active(&self) -> bool {
        self.stage != Stage::Off
    }
}
