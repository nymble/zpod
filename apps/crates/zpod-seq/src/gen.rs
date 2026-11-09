//! Explicit, seeded generators. Each returns a modified copy; the app keeps
//! the old pattern for Undo and only applies on an explicit action.

use crate::pattern::{Pattern, Step, MAX_STEPS};

/// Euclidean rhythm: `k` hits over `n` steps, rotated by `rot`.
pub fn euclid(k: u8, n: u8, rot: u8, i: u8) -> bool {
    if n == 0 || k == 0 {
        return false;
    }
    let (k, n) = (k.min(n) as u32, n as u32);
    let i = (i as u32 + n - (rot as u32 % n)) % n;
    (i * k) % n < k
}

/// Replace the on/off rhythm of `track` with E(k, length, rot). Steps that
/// stay on keep their note/vel/gate/prob; new steps get `template`.
pub fn euclid_fill(p: &Pattern, track: usize, k: u8, rot: u8) -> Pattern {
    let mut q = *p;
    let n = p.length;
    let template = p.tracks[track].steps.iter().find(|s| s.on).copied().unwrap_or(Step { on: true, ..Step::OFF });
    for i in 0..MAX_STEPS as u8 {
        let s = &mut q.tracks[track].steps[i as usize];
        let hit = i < n && euclid(k, n, rot, i);
        if hit && !s.on {
            *s = Step { on: true, ..template };
        }
        s.on = hit;
    }
    q
}

struct Rng(u32);
impl Rng {
    fn next(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }
}

/// New notes for the steps that are on (rhythm unchanged): a seeded walk
/// over scale degrees, mostly small moves, resting on chord tones (0, 2, 4)
/// on strong steps. Same seed = same notes.
pub fn walk_notes(p: &Pattern, track: usize, seed: u32) -> Pattern {
    let mut q = *p;
    let mut r = Rng(seed.wrapping_mul(2_654_435_761).wrapping_add(0x6d2b_79f5) | 1);
    let (lo, hi) = if p.tracks[track].pitched { (-3i32, 9i32) } else { (-2, 2) };
    let mut d = 0i32;
    for i in 0..MAX_STEPS {
        let s = &mut q.tracks[track].steps[i];
        if !s.on {
            continue;
        }
        let mv = match r.next() % 8 {
            0 => -2,
            1 | 2 => -1,
            3 | 4 => 1,
            5 => 2,
            6 => 4 - d.rem_euclid(7).min(4), // toward a chord tone
            _ => 0,
        };
        d = (d + mv).clamp(lo, hi);
        if i % 4 == 0 && p.tracks[track].pitched && ![0, 2, 4].contains(&d.rem_euclid(7)) {
            d = (d - 1).clamp(lo, hi);
        }
        s.note = d as i8;
    }
    q
}

pub fn clear_track(p: &Pattern, track: usize) -> Pattern {
    let mut q = *p;
    for s in q.tracks[track].steps.iter_mut() {
        s.on = false;
    }
    q
}
