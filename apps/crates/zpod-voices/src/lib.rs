// Copyright (C) 2026 Paul Lambert
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! Portable voice rack for ZPOD sequencer apps (Pipit).
//!
//! * [`def`]: data-driven voice definitions (`format = "pipit-voices"` TOML
//!   banks), strict validation, built-in default bank.
//! * [`dsp`]: small band-limited / efficient primitives (polyBLEP, TPT SVF,
//!   fast sine, envelopes, Karplus-Strong).
//! * [`rack`]: per-track voice slots with stable ids, event handling at
//!   sample offsets, smoothing, DC blocker, limiter. `render` uses only
//!   caller- and rack-owned buffers: no allocation, I/O or locks.

pub mod def;
pub mod dsp;
pub mod rack;

pub use def::{Bank, Kind, VoiceDef, VoiceInfo};
pub use rack::{shape, Rack, AUDITION, MAX_BLOCK};

#[cfg(test)]
mod tests;
