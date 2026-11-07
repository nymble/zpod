//! Pattern data and its versioned file format (TOML, `format = "pipit-pattern"`).

use crate::event::Pitch;
use serde::{Deserialize, Serialize};

pub const MAX_STEPS: usize = 16;
pub const TRACKS: usize = 5;
pub const FORMAT: &str = "pipit-pattern";
pub const VERSION: u32 = 1;
pub const TEMPO_MIN: f32 = 50.0;
pub const TEMPO_MAX: f32 = 200.0;
pub const SWING_MIN: u8 = 50;
pub const SWING_MAX: u8 = 75;

/// Short fixed-size name (voice ids, pattern names) so a Pattern is `Copy`
/// and can be handed to the audio thread without allocation.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Name([u8; 24], u8);

impl Name {
    pub fn new(s: &str) -> Self {
        let mut b = [0u8; 24];
        let mut n = 0;
        for ch in s.chars() {
            let mut tmp = [0u8; 4];
            let e = ch.encode_utf8(&mut tmp).as_bytes();
            if n + e.len() > 24 {
                break;
            }
            b[n..n + e.len()].copy_from_slice(e);
            n += e.len();
        }
        Name(b, n as u8)
    }
    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.0[..self.1 as usize]).unwrap_or("")
    }
}

impl core::fmt::Debug for Name {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:?}", self.as_str())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scale {
    Major,
    Minor,
    Dorian,
    Phrygian,
    PentaMinor,
    PentaMajor,
    Blues,
}

impl Scale {
    pub const ALL: [Scale; 7] = [Scale::Minor, Scale::Major, Scale::Dorian, Scale::Phrygian, Scale::PentaMinor, Scale::PentaMajor, Scale::Blues];
    pub fn intervals(self) -> &'static [u8] {
        match self {
            Scale::Major => &[0, 2, 4, 5, 7, 9, 11],
            Scale::Minor => &[0, 2, 3, 5, 7, 8, 10],
            Scale::Dorian => &[0, 2, 3, 5, 7, 9, 10],
            Scale::Phrygian => &[0, 1, 3, 5, 7, 8, 10],
            Scale::PentaMinor => &[0, 3, 5, 7, 10],
            Scale::PentaMajor => &[0, 2, 4, 7, 9],
            Scale::Blues => &[0, 3, 5, 6, 7, 10],
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            Scale::Major => "major",
            Scale::Minor => "minor",
            Scale::Dorian => "dorian",
            Scale::Phrygian => "phrygian",
            Scale::PentaMinor => "penta-minor",
            Scale::PentaMajor => "penta-major",
            Scale::Blues => "blues",
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Scale::Major => "Major",
            Scale::Minor => "Minor",
            Scale::Dorian => "Dorian",
            Scale::Phrygian => "Phrygian",
            Scale::PentaMinor => "Pent min",
            Scale::PentaMajor => "Pent maj",
            Scale::Blues => "Blues",
        }
    }
    pub fn from_key(s: &str) -> Option<Scale> {
        Self::ALL.iter().copied().find(|x| x.key() == s)
    }
    /// Semitones above the root for a (possibly negative) scale degree.
    pub fn semis(self, degree: i32) -> i32 {
        let iv = self.intervals();
        let n = iv.len() as i32;
        degree.div_euclid(n) * 12 + iv[degree.rem_euclid(n) as usize] as i32
    }
}

pub const ROOT_NAMES: [&str; 12] = ["C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Step {
    pub on: bool,
    /// pitched track: scale degree from the track octave's root;
    /// drum track: tuning in semitones
    pub note: i8,
    /// 1..127 (≥ 110 reads as an accent)
    pub vel: u8,
    /// note length in quarter steps, 1..16 (4 = one step)
    pub gate: u8,
    /// chance the step plays, 0..100 %
    pub prob: u8,
}

impl Step {
    pub const OFF: Step = Step { on: false, note: 0, vel: 100, gate: 2, prob: 100 };
    pub const NOTE_MIN: i8 = -14;
    pub const NOTE_MAX: i8 = 21;
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Track {
    pub voice: Name,
    pub level: u8,
    pub mute: bool,
    pub pitched: bool,
    /// octave of degree 0 on pitched tracks (C4 = octave 4)
    pub octave: i8,
    pub steps: [Step; MAX_STEPS],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pattern {
    pub name: Name,
    pub tempo: f32,
    /// 50 = straight, 66 ≈ triplet shuffle, 75 = dotted
    pub swing: u8,
    pub length: u8,
    pub scale: Scale,
    pub root: u8,
    /// seed for step probabilities (same seed = same variations)
    pub seed: u32,
    pub tracks: [Track; TRACKS],
}

impl Pattern {
    /// Empty pattern with the default kit (kick, snare, hat, bass, lead).
    pub fn empty() -> Self {
        let t = |v: &str, pitched: bool, octave: i8, level: u8| Track { voice: Name::new(v), level, mute: false, pitched, octave, steps: [Step::OFF; MAX_STEPS] };
        Pattern {
            name: Name::new("Untitled"),
            tempo: 100.0,
            swing: 50,
            length: 16,
            scale: Scale::Minor,
            root: 9,
            seed: 1,
            tracks: [t("kick.deep", false, 0, 112), t("snare.crack", false, 0, 96), t("hat.closed", false, 0, 80), t("bass.round", true, 2, 100), t("lead.pluck", true, 4, 88)],
        }
    }

    /// Pitch of a step (pitched: scale degree; drums: C4 + tuning, the
    /// voice treats 261.63 Hz as "untransposed").
    pub fn pitch(&self, track: usize, step: &Step) -> Pitch {
        let t = &self.tracks[track];
        if t.pitched {
            let n = 12 * (t.octave as i32 + 1) + self.root as i32 + self.scale.semis(step.note as i32);
            Pitch::from_note(n as f32)
        } else {
            Pitch::from_note(60.0 + step.note as f32)
        }
    }

    pub fn note_name(&self, track: usize, step: &Step) -> String {
        let t = &self.tracks[track];
        if !t.pitched {
            return format!("{:+}", step.note);
        }
        let n = 12 * (t.octave as i32 + 1) + self.root as i32 + self.scale.semis(step.note as i32);
        format!("{}{}", ROOT_NAMES[n.rem_euclid(12) as usize], n.div_euclid(12) - 1)
    }

    /// Clamp everything into range (after edits; files are validated strictly instead).
    pub fn clamp(&mut self) {
        self.tempo = self.tempo.clamp(TEMPO_MIN, TEMPO_MAX);
        self.swing = self.swing.clamp(SWING_MIN, SWING_MAX);
        self.length = self.length.clamp(1, MAX_STEPS as u8);
        self.root %= 12;
        for t in self.tracks.iter_mut() {
            t.level = t.level.min(127);
            t.octave = t.octave.clamp(0, 7);
            for s in t.steps.iter_mut() {
                s.note = s.note.clamp(Step::NOTE_MIN, Step::NOTE_MAX);
                s.vel = s.vel.clamp(1, 127);
                s.gate = s.gate.clamp(1, 16);
                s.prob = s.prob.min(100);
            }
        }
    }

    // ---- file format ----

    pub fn to_toml(&self) -> String {
        let f = File {
            format: FORMAT.into(),
            version: VERSION,
            name: self.name.as_str().into(),
            tempo: self.tempo,
            swing: self.swing,
            length: self.length,
            scale: self.scale.key().into(),
            root: self.root,
            seed: self.seed,
            track: self
                .tracks
                .iter()
                .map(|t| FileTrack {
                    voice: t.voice.as_str().into(),
                    level: t.level,
                    mute: t.mute,
                    pitched: t.pitched,
                    octave: t.octave,
                    on: t.steps.iter().map(|s| s.on as u8).collect(),
                    note: t.steps.iter().map(|s| s.note).collect(),
                    vel: t.steps.iter().map(|s| s.vel).collect(),
                    gate: t.steps.iter().map(|s| s.gate).collect(),
                    prob: t.steps.iter().map(|s| s.prob).collect(),
                })
                .collect(),
        };
        toml::to_string(&f).unwrap_or_default()
    }

    /// Strict parse: wrong format/version, unknown keys, wrong lengths or
    /// out-of-range values are errors (the caller keeps its current pattern).
    pub fn from_toml(s: &str) -> Result<Pattern, String> {
        let f: File = toml::from_str(s).map_err(|e| format!("not a pattern file: {}", e.message()))?;
        if f.format != FORMAT {
            return Err(format!("format is '{}', expected '{FORMAT}'", f.format));
        }
        if f.version == 0 || f.version > VERSION {
            return Err(format!("pattern version {} not supported (this app reads 1..={VERSION})", f.version));
        }
        let rng = |ok: bool, what: &str| if ok { Ok(()) } else { Err(format!("{what} out of range")) };
        rng((TEMPO_MIN..=TEMPO_MAX).contains(&f.tempo), "tempo")?;
        rng((SWING_MIN..=SWING_MAX).contains(&f.swing), "swing")?;
        rng((1..=MAX_STEPS as u8).contains(&f.length), "length")?;
        rng(f.root < 12, "root")?;
        let scale = Scale::from_key(&f.scale).ok_or_else(|| format!("unknown scale '{}'", f.scale))?;
        if f.track.len() != TRACKS {
            return Err(format!("{} tracks, expected {TRACKS}", f.track.len()));
        }
        let mut p = Pattern::empty();
        p.name = Name::new(&f.name);
        p.tempo = f.tempo;
        p.swing = f.swing;
        p.length = f.length;
        p.scale = scale;
        p.root = f.root;
        p.seed = f.seed;
        for (i, ft) in f.track.iter().enumerate() {
            for (n, l) in [("on", ft.on.len()), ("note", ft.note.len()), ("vel", ft.vel.len()), ("gate", ft.gate.len()), ("prob", ft.prob.len())] {
                if l != MAX_STEPS {
                    return Err(format!("track {}: {n} has {l} values, expected {MAX_STEPS}", i + 1));
                }
            }
            if ft.voice.is_empty() || ft.voice.len() > 24 {
                return Err(format!("track {}: bad voice id", i + 1));
            }
            rng(ft.level <= 127, "level")?;
            rng((0..=7).contains(&ft.octave), "octave")?;
            let t = &mut p.tracks[i];
            t.voice = Name::new(&ft.voice);
            t.level = ft.level;
            t.mute = ft.mute;
            t.pitched = ft.pitched;
            t.octave = ft.octave;
            for k in 0..MAX_STEPS {
                rng(ft.on[k] <= 1, "on")?;
                rng((Step::NOTE_MIN..=Step::NOTE_MAX).contains(&ft.note[k]), "note")?;
                rng((1..=127).contains(&ft.vel[k]), "vel")?;
                rng((1..=16).contains(&ft.gate[k]), "gate")?;
                rng(ft.prob[k] <= 100, "prob")?;
                t.steps[k] = Step { on: ft.on[k] == 1, note: ft.note[k], vel: ft.vel[k], gate: ft.gate[k], prob: ft.prob[k] };
            }
        }
        Ok(p)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    format: String,
    version: u32,
    name: String,
    tempo: f32,
    swing: u8,
    length: u8,
    scale: String,
    root: u8,
    seed: u32,
    track: Vec<FileTrack>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileTrack {
    voice: String,
    level: u8,
    mute: bool,
    pitched: bool,
    octave: i8,
    on: Vec<u8>,
    note: Vec<i8>,
    vel: Vec<u8>,
    gate: Vec<u8>,
    prob: Vec<u8>,
}
