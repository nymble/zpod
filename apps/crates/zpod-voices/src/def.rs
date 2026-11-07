//! Voice definitions and banks.

use serde::Deserialize;

pub const FORMAT: &str = "pipit-voices";
pub const VERSION: u32 = 1;
pub const DEFAULT_BANK: &str = include_str!("../banks/default.toml");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Kick,
    Snare,
    Hat,
    Clap,
    /// monophonic subtractive (bass): glide, 24 dB filter
    Mono,
    /// polyphonic subtractive (leads, pads)
    Poly,
    /// Karplus-Strong string
    Pluck,
    /// 2-operator FM (bells, keys)
    Fm,
}

impl Kind {
    pub fn from_key(s: &str) -> Option<Kind> {
        Some(match s {
            "kick" => Kind::Kick,
            "snare" => Kind::Snare,
            "hat" => Kind::Hat,
            "clap" => Kind::Clap,
            "mono" => Kind::Mono,
            "poly" => Kind::Poly,
            "pluck" => Kind::Pluck,
            "fm" => Kind::Fm,
            _ => return None,
        })
    }
    pub fn is_drum(self) -> bool {
        matches!(self, Kind::Kick | Kind::Snare | Kind::Hat | Kind::Clap)
    }
    /// voices per track for this kind
    pub fn polyphony(self) -> usize {
        match self {
            Kind::Mono => 1,
            k if k.is_drum() => 2, // overlap the tail of the previous hit (no click)
            _ => 4,
        }
    }
}

/// One voice. Plain numbers so the audio thread can copy it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VoiceDef {
    pub id: zpod_seq::pattern::Name,
    pub name: zpod_seq::pattern::Name,
    pub kind: Kind,
    pub level: f32,
    pub pitch: f32,
    pub sweep: f32,
    pub sweep_time: f32,
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    pub tone: f32,
    pub noise: f32,
    pub cutoff: f32,
    pub reso: f32,
    pub env_amt: f32,
    pub drive: f32,
    pub glide: f32,
    pub detune: f32,
    pub wave: f32,
    pub ratio: f32,
    pub index: f32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileBank {
    format: String,
    version: u32,
    voice: Vec<FileVoice>,
}

macro_rules! file_voice {
    ($($f:ident = $d:expr, $lo:expr, $hi:expr;)*) => {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct FileVoice {
            id: String,
            name: String,
            kind: String,
            $( #[serde(default)] $f: Option<f32>, )*
        }
        fn build(v: &FileVoice) -> Result<VoiceDef, String> {
            let kind = Kind::from_key(&v.kind).ok_or_else(|| format!("voice '{}': unknown kind '{}'", v.id, v.kind))?;
            if v.id.is_empty() || v.id.len() > 24 || !v.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_') {
                return Err(format!("voice id '{}': 1-24 chars of a-z 0-9 . - _", v.id));
            }
            $(
                let $f = v.$f.unwrap_or($d);
                if !($f.is_finite() && $f >= $lo && $f <= $hi) {
                    return Err(format!("voice '{}': {} = {} (allowed {}..{})", v.id, stringify!($f), $f, $lo, $hi));
                }
            )*
            Ok(VoiceDef { id: zpod_seq::pattern::Name::new(&v.id), name: zpod_seq::pattern::Name::new(&v.name), kind, $($f,)* })
        }
        /// (name, default, min, max) of every numeric field, for docs and UI.
        pub const FIELDS: &[(&str, f32, f32, f32)] = &[$((stringify!($f), $d, $lo, $hi),)*];
    };
}

file_voice! {
    level = 0.8, 0.0, 1.5;
    pitch = 100.0, 20.0, 4000.0;
    sweep = 1.0, 1.0, 10.0;
    sweep_time = 0.03, 0.001, 0.5;
    attack = 0.003, 0.0, 4.0;
    decay = 0.3, 0.005, 8.0;
    sustain = 0.7, 0.0, 1.0;
    release = 0.1, 0.003, 8.0;
    tone = 0.5, 0.0, 1.0;
    noise = 0.5, 0.0, 1.0;
    cutoff = 2000.0, 30.0, 18000.0;
    reso = 0.2, 0.0, 0.95;
    env_amt = 0.3, 0.0, 1.0;
    drive = 0.0, 0.0, 1.0;
    glide = 0.0, 0.0, 1.0;
    detune = 0.0, 0.0, 50.0;
    wave = 0.0, 0.0, 1.0;
    ratio = 1.0, 0.25, 16.0;
    index = 1.0, 0.0, 12.0;
}

pub const MAX_VOICES_IN_BANK: usize = 64;

/// A validated set of voices (built once, outside the audio thread).
#[derive(Clone, Debug)]
pub struct Bank {
    pub voices: Vec<VoiceDef>,
}

impl Bank {
    pub fn parse(s: &str) -> Result<Bank, String> {
        let f: FileBank = toml::from_str(s).map_err(|e| format!("not a voice bank: {}", e.message()))?;
        if f.format != FORMAT {
            return Err(format!("format is '{}', expected '{FORMAT}'", f.format));
        }
        if f.version == 0 || f.version > VERSION {
            return Err(format!("voice bank version {} not supported (reads 1..={VERSION})", f.version));
        }
        if f.voice.is_empty() || f.voice.len() > MAX_VOICES_IN_BANK {
            return Err(format!("{} voices (1..={MAX_VOICES_IN_BANK})", f.voice.len()));
        }
        let mut voices = Vec::with_capacity(f.voice.len());
        for v in &f.voice {
            let d = build(v)?;
            if voices.iter().any(|x: &VoiceDef| x.id == d.id) {
                return Err(format!("duplicate voice id '{}'", v.id));
            }
            voices.push(d);
        }
        Ok(Bank { voices })
    }
    pub fn builtin() -> Bank {
        Bank::parse(DEFAULT_BANK).expect("built-in bank is valid")
    }
    pub fn find(&self, id: &str) -> Option<usize> {
        self.voices.iter().position(|v| v.id.as_str() == id)
    }
    /// next/previous voice id (wrapping), optionally only drums or only tuned
    pub fn cycle(&self, id: &str, dir: i32, drums: bool) -> Option<usize> {
        let list: Vec<usize> = (0..self.voices.len()).filter(|&i| self.voices[i].kind.is_drum() == drums).collect();
        if list.is_empty() {
            return None;
        }
        let cur = list.iter().position(|&i| self.voices[i].id.as_str() == id).unwrap_or(0) as i32;
        let n = list.len() as i32;
        Some(list[(cur + dir).rem_euclid(n) as usize])
    }
}
