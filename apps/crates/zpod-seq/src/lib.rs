//! Portable step sequencer for ZPOD (Pipit and later apps).
//!
//! * [`event`]: timestamped events (frame offset inside a block) that any
//!   voice rack can consume: note on/off with an absolute pitch in Hz (so
//!   cents and non-12-TET work), all-notes-off.
//! * [`pattern`]: the musical data (tracks × 16 steps, tempo, swing, scale)
//!   plus the versioned TOML file format and its validation.
//! * [`clock`]: the transport. Turns a pattern into events block by block;
//!   step times are computed from the step index (never accumulated), so
//!   there is no drift, and every note-on has its note-off scheduled.
//! * [`gen`]: explicit, seeded, repeatable generators (Euclidean fill,
//!   quantized note walk, clear). They return a new pattern; callers decide
//!   when to apply it (undo stays possible).
//!
//! Nothing here allocates in `Clock::process`, does file I/O or locks.

pub mod clock;
pub mod demos;
pub mod event;
pub mod gen;
pub mod pattern;

pub use clock::Clock;
pub use event::{Event, EventBuf, EventKind, Pitch};
pub use pattern::{Macros, Pattern, Rate, Scale, Step, Track, MAX_STEPS, TRACKS};
#[cfg(test)]
mod tests;
