//! Scales, harmony modes, voicing and Music Mouse grid → pitch mapping.
//! Two octaves per axis by default: lower C2–C4, upper C4–C6.

/// MIDI note numbers.
pub type Midi = u8;

pub const C2: Midi = 36;
pub const C4: Midi = 60;
pub const C6: Midi = 84;

/// Steps along one Music Mouse axis (two diatonic octaves + tonic = 15).
pub const AXIS_STEPS: i32 = 15;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Chromatic,
    Diatonic,
    Pentatonic,
    Octatonic,
    MiddleEast,
    Quartal,
}

impl Mode {
    pub const ALL: [Mode; 6] = [
        Mode::Chromatic,
        Mode::Diatonic,
        Mode::Pentatonic,
        Mode::Octatonic,
        Mode::MiddleEast,
        Mode::Quartal,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Mode::Chromatic => "Chromatic",
            Mode::Diatonic => "Diatonic",
            Mode::Pentatonic => "Pentatonic",
            Mode::Octatonic => "Octatonic",
            Mode::MiddleEast => "Middle East",
            Mode::Quartal => "Quartal",
        }
    }

    pub fn short(self) -> &'static str {
        match self {
            Mode::Chromatic => "Chrom",
            Mode::Diatonic => "Diatonic",
            Mode::Pentatonic => "Penta",
            Mode::Octatonic => "Octa",
            Mode::MiddleEast => "Hijaz",
            Mode::Quartal => "Quartal",
        }
    }

    /// Pitch-class set relative to the root (0 = root).
    pub fn intervals(self) -> &'static [u8] {
        match self {
            Mode::Chromatic => &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
            Mode::Diatonic => &[0, 2, 4, 5, 7, 9, 11],
            Mode::Pentatonic => &[0, 2, 4, 7, 9],
            Mode::Octatonic => &[0, 2, 3, 5, 6, 8, 9, 11],
            Mode::MiddleEast => &[0, 1, 4, 5, 7, 8, 10],
            Mode::Quartal => &[0, 5, 10, 3],
        }
    }

    pub fn next(self) -> Mode {
        let i = Self::ALL.iter().position(|&m| m == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }

    /// Preferred interval (in scale degrees) between the pair of voices on one axis.
    pub fn pair_degrees(self) -> i32 {
        match self {
            Mode::Quartal | Mode::Pentatonic => 1,
            _ => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Voicing {
    Parallel,
    Contrary,
    Oblique,
}

impl Voicing {
    pub const ALL: [Voicing; 3] = [Voicing::Parallel, Voicing::Contrary, Voicing::Oblique];

    pub fn name(self) -> &'static str {
        match self {
            Voicing::Parallel => "Parallel",
            Voicing::Contrary => "Contrary",
            Voicing::Oblique => "Oblique",
        }
    }

    pub fn next(self) -> Voicing {
        let i = Self::ALL.iter().position(|&v| v == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }

    pub fn prev(self) -> Voicing {
        let i = Self::ALL.iter().position(|&v| v == self).unwrap_or(0);
        Self::ALL[(i + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pattern {
    Off,
    Up,
    Down,
    UpDown,
    Chord,
}

impl Pattern {
    pub fn name(self) -> &'static str {
        match self {
            Pattern::Off => "Off",
            Pattern::Up => "Arp up",
            Pattern::Down => "Arp dn",
            Pattern::UpDown => "Arp ud",
            Pattern::Chord => "Chord",
        }
    }

    pub fn toggle(self) -> Pattern {
        match self {
            Pattern::Off => Pattern::Up,
            Pattern::Up => Pattern::Down,
            Pattern::Down => Pattern::UpDown,
            Pattern::UpDown => Pattern::Chord,
            Pattern::Chord => Pattern::Off,
        }
    }

    pub fn is_on(self) -> bool {
        self != Pattern::Off
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Root {
    /// 0 = C … 11 = B
    pub pc: u8,
}

impl Root {
    pub const C: Root = Root { pc: 0 };

    pub fn name(self) -> &'static str {
        ["C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B"][self.pc as usize % 12]
    }

    pub fn transpose(self, semitones: i32) -> Root {
        Root {
            pc: (self.pc as i32 + semitones).rem_euclid(12) as u8,
        }
    }
}

/// Map a scale degree (0..) into a MIDI note starting at `base`, using `mode` and `root`.
pub fn degree_to_midi(degree: i32, mode: Mode, root: Root, base: Midi) -> Midi {
    let iv = mode.intervals();
    let n = iv.len() as i32;
    let deg = degree.max(0);
    let octave = deg / n;
    let step = (deg % n) as usize;
    let base_pc = base as i32 % 12;
    let mut aligned = base as i32 - (base_pc - root.pc as i32).rem_euclid(12);
    if aligned > base as i32 {
        aligned -= 12;
    }
    let midi = aligned + octave * 12 + iv[step] as i32;
    midi.clamp(0, 127) as Midi
}

#[derive(Clone, Copy, Debug)]
pub struct Mapper {
    pub mode: Mode,
    pub root: Root,
    pub voicing: Voicing,
    pub transpose: i32,
    pub lower_base: Midi,
    pub upper_base: Midi,
}

impl Default for Mapper {
    fn default() -> Self {
        Self {
            mode: Mode::Diatonic,
            root: Root::C,
            voicing: Voicing::Contrary,
            transpose: 0,
            lower_base: C2,
            upper_base: C4,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chord4 {
    /// V1 lower secondary, V2 lower primary, V3 upper secondary, V4 upper primary
    pub notes: [Midi; 4],
}

impl Mapper {
    pub fn pitch_at(&self, axis_base: Midi, degree: i32) -> Midi {
        let m = degree_to_midi(degree, self.mode, self.root, axis_base);
        (m as i32 + self.transpose).clamp(0, 127) as Midi
    }

    pub fn chord(&self, x: i32, y: i32) -> Chord4 {
        let x = x.clamp(0, AXIS_STEPS - 1);
        let y = y.clamp(0, AXIS_STEPS - 1);
        let gap = self.mode.pair_degrees();

        let lower_pri = self.pitch_at(self.lower_base, x);
        let lower_sec = self.pitch_at(self.lower_base, (x - gap).max(0));

        let (upper_pri_deg, upper_sec_deg) = match self.voicing {
            Voicing::Parallel => (y, (y - gap).max(0)),
            Voicing::Contrary => {
                let cy = (AXIS_STEPS - 1) - y;
                (cy, (cy - gap).max(0))
            }
            Voicing::Oblique => {
                let hold = AXIS_STEPS / 2;
                (y, hold)
            }
        };
        let upper_pri = self.pitch_at(self.upper_base, upper_pri_deg);
        let upper_sec = self.pitch_at(self.upper_base, upper_sec_deg);

        Chord4 {
            notes: [lower_sec, lower_pri, upper_sec, upper_pri],
        }
    }
}

pub fn note_name_parts(midi: Midi) -> (&'static str, i32) {
    const NAMES: [&str; 12] = ["C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B"];
    let pc = NAMES[(midi % 12) as usize];
    let oct = (midi as i32 / 12) - 1;
    (pc, oct)
}

/// Format into a caller buffer ("C4", "F#5", …); returns the written prefix.
pub fn format_note(midi: Midi, buf: &mut [u8]) -> &str {
    let (pc, oct) = note_name_parts(midi);
    let mut n = 0;
    for b in pc.bytes() {
        if n < buf.len() {
            buf[n] = b;
            n += 1;
        }
    }
    if oct < 0 && n + 1 < buf.len() {
        buf[n] = b'-';
        n += 1;
        buf[n] = b'0' + (-oct) as u8;
        n += 1;
    } else if (0..10).contains(&oct) && n < buf.len() {
        buf[n] = b'0' + oct as u8;
        n += 1;
    }
    core::str::from_utf8(&buf[..n]).unwrap_or("?")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diatonic_c_degrees() {
        assert_eq!(degree_to_midi(0, Mode::Diatonic, Root::C, C2), 36);
        assert_eq!(degree_to_midi(1, Mode::Diatonic, Root::C, C2), 38);
        assert_eq!(degree_to_midi(2, Mode::Diatonic, Root::C, C2), 40);
        assert_eq!(degree_to_midi(7, Mode::Diatonic, Root::C, C2), 48);
        assert_eq!(degree_to_midi(14, Mode::Diatonic, Root::C, C2), 60);
    }

    #[test]
    fn pentatonic_shorter() {
        assert_eq!(degree_to_midi(0, Mode::Pentatonic, Root::C, C4), 60);
        assert_eq!(degree_to_midi(1, Mode::Pentatonic, Root::C, C4), 62);
        assert_eq!(degree_to_midi(4, Mode::Pentatonic, Root::C, C4), 69);
        assert_eq!(degree_to_midi(5, Mode::Pentatonic, Root::C, C4), 72);
    }

    #[test]
    fn transpose_root() {
        assert_eq!(Root::C.transpose(2).name(), "D");
        assert_eq!(Root::C.transpose(-1).name(), "B");
    }

    #[test]
    fn contrary_mirrors_y() {
        let m = Mapper {
            voicing: Voicing::Contrary,
            ..Mapper::default()
        };
        let low = m.chord(0, 0);
        let high = m.chord(0, AXIS_STEPS - 1);
        assert!(high.notes[3] < low.notes[3]);
    }

    #[test]
    fn parallel_follows_y() {
        let m = Mapper {
            voicing: Voicing::Parallel,
            ..Mapper::default()
        };
        let a = m.chord(7, 3);
        let b = m.chord(7, 10);
        assert!(b.notes[3] > a.notes[3]);
        assert_eq!(a.notes[1], b.notes[1]);
    }

    #[test]
    fn mode_cycle() {
        let mut m = Mode::Chromatic;
        let start = m;
        for _ in 0..6 {
            m = m.next();
        }
        assert_eq!(m, start);
    }

    #[test]
    fn note_names() {
        let mut buf = [0u8; 4];
        assert_eq!(format_note(60, &mut buf), "C4");
        assert_eq!(format_note(61, &mut buf), "C#4");
        assert_eq!(format_note(36, &mut buf), "C2");
    }

    #[test]
    fn two_octaves_cover() {
        let m = Mapper::default();
        let lo = m.chord(0, 0);
        let hi = m.chord(AXIS_STEPS - 1, AXIS_STEPS - 1);
        assert!(lo.notes[1] <= C2 + 1);
        assert!(hi.notes[1] >= C4 - 2);
        let mp = Mapper {
            voicing: Voicing::Parallel,
            ..Mapper::default()
        };
        let top = mp.chord(0, AXIS_STEPS - 1);
        assert!(top.notes[3] >= 80);
    }
}
