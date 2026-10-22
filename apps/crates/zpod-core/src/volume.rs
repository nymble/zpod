//! Side volume buttons: shared state file + the one on-screen overlay.
//!
//! `zpod-buttons` (the button daemon) is the only writer of the ALSA
//! 'Digital' volume. After every step it writes `/run/zpod/volume`:
//!
//! ```text
//! <percent> <held>\n        e.g. "46 1\n"  (held = a volume key is down)
//! ```
//!
//! and only then emits KEY_VOLUMEUP / KEY_VOLUMEDOWN (press, one repeat per
//! step while held, release). Apps never change the volume themselves and
//! never run amixer for it; they read the file. The overlay below is drawn
//! by the platform layer for every app, so it looks the same everywhere.

use crate::{Event, Key, KeyState};

/// Approved defaults (Howard Qiao's zpod/settings.ini, adopted 2026-10-22).
pub const VOLUME_CAP_PCT: u8 = 80;
pub const VOLUME_STEP_PCT: u8 = 2;
/// A press shorter than this is ignored (contact bounce / brush).
pub const KEY_SHORT_MIN_MS: u32 = 75;
/// Held longer than this = long press; volume starts repeating here.
pub const KEY_SHORT_MAX_MS: u32 = 650;
/// Long-press actions fire by this time at the latest.
pub const KEY_LONG_MAX_MS: u32 = 1200;
/// Steady repeat period while a volume key stays down.
pub const VOLUME_REPEAT_MS: u32 = 150;
/// Overlay stays this long after the last volume key is released.
pub const OVERLAY_LINGER_MS: u64 = 1200;

pub const STATE_FILE: &str = "/run/zpod/volume";

/// Parse the state file: (percent, held).
pub fn parse_state(s: &str) -> Option<(u8, bool)> {
    let mut it = s.split_whitespace();
    let pct: u8 = it.next()?.parse().ok()?;
    let held = it.next().map(|h| h == "1").unwrap_or(false);
    Some((pct.min(100), held))
}

/// Next volume for one step (what the daemon applies), clamped to [0, cap].
pub fn step(pct: u8, up: bool, step: u8, cap: u8) -> u8 {
    let p = pct.min(cap) as i16;
    let n = if up { p + step as i16 } else { p - step as i16 };
    n.clamp(0, cap as i16) as u8
}

/// Visibility logic of the volume overlay. Time is any monotonic ms counter.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VolumeOverlay {
    pub pct: Option<u8>,
    /// Volume keys currently held down.
    held: u8,
    hide_at: Option<u64>,
    visible: bool,
    /// Something to redraw (value or visibility changed).
    pub dirty: bool,
}

impl VolumeOverlay {
    pub fn visible(&self) -> bool {
        self.visible
    }

    /// Feed every input event; returns true if it was a volume key.
    /// `pct` = current volume from the state file (None = unknown).
    pub fn on_event(&mut self, ev: &Event, pct: Option<u8>, now_ms: u64) -> bool {
        let bit = match ev.key {
            Key::VolUp => 1,
            Key::VolDown => 2,
            _ => return false,
        };
        match ev.state {
            KeyState::Press | KeyState::Repeat => {
                self.held |= bit;
                self.hide_at = None;
            }
            KeyState::Release => {
                self.held &= !bit;
                if self.held == 0 {
                    self.hide_at = Some(now_ms + OVERLAY_LINGER_MS);
                }
            }
        }
        if pct.is_some() && pct != self.pct {
            self.pct = pct;
            self.dirty = true;
        }
        if !self.visible {
            self.visible = true;
            self.dirty = true;
        }
        true
    }

    /// Advance time; returns true when the overlay just disappeared.
    pub fn tick(&mut self, now_ms: u64) -> bool {
        if self.visible && self.held == 0 && self.hide_at.is_some_and(|t| now_ms >= t) {
            self.visible = false;
            self.hide_at = None;
            self.dirty = true;
            return true;
        }
        false
    }

    /// Milliseconds until the overlay hides (for poll timeouts).
    pub fn ms_until_hide(&self, now_ms: u64) -> Option<u64> {
        if !self.visible {
            return None;
        }
        self.hide_at.map(|t| t.saturating_sub(now_ms))
    }
}

/// Battery icon level, 6 steps: 0 = empty (<5 %, shutdown zone), 1..=5 bars.
pub fn battery_level6(pct: f32) -> u8 {
    match pct {
        p if p < 5.0 => 0,
        p if p < 20.0 => 1,
        p if p < 40.0 => 2,
        p if p < 60.0 => 3,
        p if p < 80.0 => 4,
        _ => 5,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(key: Key, state: KeyState) -> Event {
        Event { key, state }
    }

    #[test]
    fn state_file_parse() {
        assert_eq!(parse_state("46 1\n"), Some((46, true)));
        assert_eq!(parse_state("80 0"), Some((80, false)));
        assert_eq!(parse_state("12"), Some((12, false)));
        assert_eq!(parse_state(""), None);
        assert_eq!(parse_state("x 1"), None);
    }

    #[test]
    fn step_is_single_and_capped() {
        assert_eq!(step(40, true, 2, 80), 42);
        assert_eq!(step(79, true, 2, 80), 80);
        assert_eq!(step(80, true, 2, 80), 80);
        assert_eq!(step(95, false, 2, 80), 78); // above cap (old M3 default) comes down from the cap
        assert_eq!(step(1, false, 2, 80), 0);
        assert_eq!(step(0, false, 2, 80), 0);
    }

    #[test]
    fn overlay_stays_while_held_and_lingers_after_release() {
        let mut o = VolumeOverlay::default();
        assert!(o.on_event(&ev(Key::VolUp, KeyState::Press), Some(40), 0));
        assert!(o.visible());
        // held for 5 s with repeats: never hides
        for t in (150..5000).step_by(150) {
            o.on_event(&ev(Key::VolUp, KeyState::Repeat), Some(40 + (t / 150) as u8), t);
            assert!(!o.tick(t + 10));
            assert!(o.visible());
        }
        // even a long gap without repeats while still held does not hide it
        assert!(!o.tick(60_000));
        o.on_event(&ev(Key::VolUp, KeyState::Release), None, 60_000);
        assert!(!o.tick(60_000 + OVERLAY_LINGER_MS - 1));
        assert!(o.tick(60_000 + OVERLAY_LINGER_MS));
        assert!(!o.visible());
    }

    #[test]
    fn overlay_ignores_other_keys() {
        let mut o = VolumeOverlay::default();
        assert!(!o.on_event(&ev(Key::Up, KeyState::Press), Some(10), 0));
        assert!(!o.visible());
    }

    #[test]
    fn both_keys_released_before_hiding() {
        let mut o = VolumeOverlay::default();
        o.on_event(&ev(Key::VolUp, KeyState::Press), Some(10), 0);
        o.on_event(&ev(Key::VolDown, KeyState::Press), Some(10), 10);
        o.on_event(&ev(Key::VolUp, KeyState::Release), None, 20);
        assert!(!o.tick(10_000));
        o.on_event(&ev(Key::VolDown, KeyState::Release), None, 10_000);
        assert_eq!(o.ms_until_hide(10_000), Some(OVERLAY_LINGER_MS));
        assert!(o.tick(10_000 + OVERLAY_LINGER_MS));
    }

    #[test]
    fn battery_six_steps() {
        let l: [u8; 8] = [0.0, 4.9, 5.0, 19.9, 39.0, 59.0, 79.0, 100.0].map(battery_level6);
        assert_eq!(l, [0, 0, 1, 1, 2, 3, 4, 5]);
    }
}
