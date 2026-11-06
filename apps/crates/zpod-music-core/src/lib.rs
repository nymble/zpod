//! Shared engine for ZPOD generative music apps (Mouse Music and later).
//!
//! * [`harmony`] — scales, modes, voicing, Music Mouse grid → MIDI
//! * [`synth`] — subtractive voices + allocator
//! * [`audio`] — exclusive ALSA (via aplay), MPD pause/restore, fast-arrows request, `--bench`

pub mod audio;
pub mod harmony;
pub mod synth;

pub use audio::{
    bench_cpu, AudioRuntime, BenchResult, FastArrowsGuard, MpdExclusive, PerfState, SharedPerf,
    BUTTONS_MODE_PATH, PERIOD_FRAMES, SAMPLE_RATE,
};
pub use harmony::{
    format_note, Chord4, Mapper, Mode, Pattern, Root, Voicing, AXIS_STEPS, C2, C4, C6,
};
pub use synth::{Engine, MAX_VOICES};
