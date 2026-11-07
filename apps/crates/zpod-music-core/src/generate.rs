//! Deterministic generative sequencer (Tidepool): four Euclidean lanes over
//! a scale that drift slowly. Pure logic, no audio or I/O, so it is portable
//! and unit-tested; apps drive it one 1/16 step at a time and feed the
//! resulting note events to any synth (`synth::Engine`).
//!
//! The same seed, scale, root and tempo always give the same sequence.
//! Every note has a fixed length in steps, so nothing can hang: `stop()`
//! returns note-offs for everything still held.

use crate::harmony::{degree_to_midi, Midi, Mode, Root};

/// xorshift32: tiny, deterministic, good enough for musical choices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Self {
        // splitmix-style scramble so seeds 1, 2, 3 differ a lot; never 0
        let mut z = seed.wrapping_add(0x9E37_79B9);
        z = (z ^ (z >> 16)).wrapping_mul(0x85EB_CA6B);
        z = (z ^ (z >> 13)).wrapping_mul(0xC2B2_AE35);
        z ^= z >> 16;
        Rng(if z == 0 { 0x1234_5678 } else { z })
    }
    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }
    /// 0..n (n > 0)
    pub fn below(&mut self, n: u32) -> u32 {
        self.next_u32() % n.max(1)
    }
    /// true with probability pct/100
    pub fn chance(&mut self, pct: u32) -> bool {
        self.below(100) < pct
    }
}

/// Bjorklund-equivalent Euclidean rhythm: `k` onsets spread over `n` steps,
/// rotated by `rot`. `euclid(3, 8, 0)` = x..x..x.
pub fn euclid_hit(k: u8, n: u8, rot: u8, step: u32) -> bool {
    if n == 0 || k == 0 {
        return false;
    }
    let k = k.min(n) as u32;
    let n = n as u32;
    let i = (step + rot as u32) % n;
    // onset where floor(i*k/n) changes (Bresenham form)
    (i * k) % n < k
}

/// Scales offered by the generator (subset of `harmony::Mode` that stays calm).
pub const SCALES: [Mode; 4] = [Mode::Pentatonic, Mode::Diatonic, Mode::Quartal, Mode::MiddleEast];

pub fn scale_name(m: Mode) -> &'static str {
    match m {
        Mode::Pentatonic => "Pentatonic",
        Mode::Diatonic => "Diatonic",
        Mode::Quartal => "Quartal",
        Mode::MiddleEast => "Hijaz",
        other => other.short(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoteEvent {
    On(Midi, u8),
    Off(Midi),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lane {
    pub name: &'static str,
    /// onsets and cycle length (polymetric: lanes have different n)
    pub k: u8,
    pub n: u8,
    pub rot: u8,
    /// current scale degree and allowed range
    pub degree: i32,
    lo: i32,
    hi: i32,
    base: Midi,
    /// note length in steps, velocity 0..127, chance a hit sounds (%)
    len: u8,
    vel: u8,
    prob: u8,
    k_min: u8,
    k_max: u8,
}

pub const LANES: usize = 4;
/// Most notes held at once (drone pair + one per other lane).
pub const MAX_HELD: usize = 6;

#[derive(Clone, Copy, Debug)]
struct Held {
    midi: Midi,
    left: u16,
}

/// The sequencer. Call [`Tidepool::step`] once per 1/16 note.
#[derive(Clone, Debug)]
pub struct Tidepool {
    pub seed: u32,
    pub scale: Mode,
    pub root: Root,
    pub lanes: [Lane; LANES],
    pub step_no: u32,
    rng: Rng,
    held: [Option<Held>; MAX_HELD],
    /// last step each lane sounded (for the UI)
    pub last_hit: [Option<u32>; LANES],
}

impl Tidepool {
    pub fn new(seed: u32, scale: Mode, root: Root) -> Self {
        let mut rng = Rng::new(seed);
        let mut r = |lo: u8, hi: u8| lo + rng.below((hi - lo + 1) as u32) as u8;
        let lanes = [
            // low drone: root and fifth, very long notes
            Lane { name: "Drone", k: 1, n: 32, rot: 0, degree: 0, lo: 0, hi: 0, base: 36, len: 30, vel: 70, prob: 100, k_min: 1, k_max: 1 },
            Lane { name: "Pulse", k: r(3, 5), n: 16, rot: r(0, 15), degree: 2, lo: 0, hi: 6, base: 48, len: 3, vel: 72, prob: 90, k_min: 2, k_max: 6 },
            Lane { name: "Bell", k: r(2, 5), n: 13, rot: r(0, 12), degree: 4, lo: 2, hi: 10, base: 72, len: 2, vel: 58, prob: 75, k_min: 2, k_max: 6 },
            Lane { name: "Glass", k: r(2, 4), n: 9, rot: r(0, 8), degree: 3, lo: 0, hi: 8, base: 60, len: 5, vel: 52, prob: 60, k_min: 1, k_max: 5 },
        ];
        Self { seed, scale, root, lanes, step_no: 0, rng, held: [None; MAX_HELD], last_hit: [None; LANES] }
    }

    pub fn held_count(&self) -> usize {
        self.held.iter().filter(|h| h.is_some()).count()
    }

    fn is_held(&self, m: Midi) -> bool {
        self.held.iter().flatten().any(|h| h.midi == m)
    }

    fn hold(&mut self, m: Midi, len: u8, vel: u8, out: &mut Vec<NoteEvent>) -> bool {
        if self.is_held(m) {
            return false; // same pitch from two lanes: one voice only (no stuck notes)
        }
        if let Some(slot) = self.held.iter_mut().find(|h| h.is_none()) {
            *slot = Some(Held { midi: m, left: len.max(1) as u16 });
            out.push(NoteEvent::On(m, vel));
            true
        } else {
            false
        }
    }

    /// Slow evolution at bar lines: melodic random walk; every 4 bars one
    /// lane changes its rhythm a little.
    fn evolve(&mut self) {
        for i in 1..LANES {
            let l = &mut self.lanes[i];
            let d = match self.rng.below(5) { 0 => -1, 1 => 1, _ => 0 };
            l.degree = (l.degree + d).clamp(l.lo, l.hi);
        }
        if (self.step_no / 16) % 4 == 3 {
            let i = 1 + self.rng.below((LANES - 1) as u32) as usize;
            let up = self.rng.chance(50);
            let l = &mut self.lanes[i];
            if self.rng.chance(60) {
                l.k = if up { (l.k + 1).min(l.k_max) } else { l.k.saturating_sub(1).max(l.k_min) };
            } else {
                l.rot = (l.rot + 1) % l.n;
            }
        }
    }

    /// Advance one 1/16 step; appends note-offs (first) then note-ons.
    pub fn step(&mut self, out: &mut Vec<NoteEvent>) {
        // 1. end notes whose time is up
        for h in self.held.iter_mut() {
            if let Some(x) = h {
                x.left = x.left.saturating_sub(1);
                if x.left == 0 {
                    out.push(NoteEvent::Off(x.midi));
                    *h = None;
                }
            }
        }
        let s = self.step_no;
        if s % 16 == 0 && s > 0 {
            self.evolve();
        }
        // 2. new notes
        for i in 0..LANES {
            let l = self.lanes[i];
            if !euclid_hit(l.k, l.n, l.rot, s) {
                continue;
            }
            let sounds = self.rng.chance(l.prob as u32);
            if !sounds {
                continue;
            }
            let jitter = self.rng.below(12) as u8;
            let vel = l.vel.saturating_sub(jitter);
            let m = degree_to_midi(l.degree, self.scale, self.root, l.base);
            let mut hit = self.hold(m, l.len, vel, out);
            if i == 0 {
                // drone fifth (or the scale's nearest degree) above the root
                let fifth = degree_to_midi(if self.scale.intervals().len() >= 5 { self.scale.intervals().iter().position(|&x| x == 7).unwrap_or(2) as i32 } else { 1 }, self.scale, self.root, l.base);
                hit |= self.hold(fifth, l.len, vel.saturating_sub(10), out);
            }
            if hit {
                self.last_hit[i] = Some(s);
            }
        }
        self.step_no = s.wrapping_add(1);
    }

    /// Note-offs for everything held (Stop, exit, scale change).
    pub fn stop(&mut self, out: &mut Vec<NoteEvent>) {
        for h in self.held.iter_mut() {
            if let Some(x) = h.take() {
                out.push(NoteEvent::Off(x.midi));
            }
        }
    }
}

/// Samples per 1/16 step at `bpm`.
pub fn step_samples(sample_rate: u32, bpm: u32) -> u32 {
    (sample_rate as u64 * 60 / (bpm.clamp(30, 200) as u64 * 4)) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(seed: u32, steps: u32) -> Vec<(u32, NoteEvent)> {
        let mut t = Tidepool::new(seed, Mode::Pentatonic, Root::C);
        let mut v = Vec::new();
        let mut buf = Vec::new();
        for s in 0..steps {
            buf.clear();
            t.step(&mut buf);
            v.extend(buf.iter().map(|e| (s, *e)));
        }
        v
    }

    #[test]
    fn euclid_counts() {
        for (k, n) in [(3u8, 8u8), (5, 16), (2, 13), (4, 9), (1, 32)] {
            for rot in 0..n {
                let c = (0..n as u32).filter(|&s| euclid_hit(k, n, rot, s)).count();
                assert_eq!(c, k as usize, "E({k},{n}) rot {rot}");
            }
        }
        let p: String = (0..8).map(|s| if euclid_hit(3, 8, 0, s) { 'x' } else { '.' }).collect();
        assert_eq!(p.matches('x').count(), 3);
    }

    #[test]
    fn deterministic_per_seed() {
        assert_eq!(run(42, 2000), run(42, 2000));
        assert_ne!(run(42, 2000), run(43, 2000));
    }

    #[test]
    fn no_stuck_notes_and_bounded_polyphony() {
        for seed in 0..50 {
            let mut t = Tidepool::new(seed, SCALES[(seed % 4) as usize], Root { pc: (seed % 12) as u8 });
            let mut on: Vec<Midi> = Vec::new();
            let mut buf = Vec::new();
            let mut since_on = std::collections::HashMap::new();
            for s in 0..5000u32 {
                buf.clear();
                t.step(&mut buf);
                for e in &buf {
                    match *e {
                        NoteEvent::On(m, v) => {
                            assert!(!on.contains(&m), "seed {seed}: double on {m}");
                            assert!(v > 0 && v <= 127);
                            on.push(m);
                            since_on.insert(m, s);
                        }
                        NoteEvent::Off(m) => {
                            let i = on.iter().position(|&x| x == m).expect("off without on");
                            on.remove(i);
                        }
                    }
                }
                assert!(on.len() <= MAX_HELD);
                assert!(on.len() <= crate::synth::MAX_VOICES);
                for (&m, &t0) in &since_on {
                    if on.contains(&m) {
                        assert!(s - t0 <= 32, "seed {seed}: note {m} held too long");
                    }
                }
            }
            buf.clear();
            t.stop(&mut buf);
            for e in &buf {
                if let NoteEvent::Off(m) = *e {
                    on.retain(|&x| x != m);
                }
            }
            assert!(on.is_empty() && t.held_count() == 0, "seed {seed}: stuck after stop");
        }
    }

    #[test]
    fn evolves_but_stays_in_range() {
        let mut t = Tidepool::new(7, Mode::Diatonic, Root::C);
        let start = t.lanes;
        let mut buf = Vec::new();
        for _ in 0..16 * 64 {
            t.step(&mut buf);
        }
        assert_ne!(start, t.lanes, "64 bars should change something");
        for l in &t.lanes {
            assert!(l.k >= 1 && l.k <= l.n && l.rot < l.n && l.degree >= l.lo && l.degree <= l.hi);
        }
        // all pitches are in the scale
        for e in buf {
            if let NoteEvent::On(m, _) = e {
                assert!(Mode::Diatonic.intervals().contains(&((m % 12) as u8)));
            }
        }
    }

    #[test]
    fn tempo_steps() {
        assert_eq!(step_samples(44_100, 60), 11_025);
        assert_eq!(step_samples(44_100, 1), step_samples(44_100, 30));
    }
}
