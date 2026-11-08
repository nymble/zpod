//! Built-in demo grooves (editable starting points, loaded only on request).

use crate::pattern::{Name, Pattern, Rate, Scale, Step, MAX_STEPS};

/// Drum row: `X` accent, `x` normal, `o` ghost, `?` 50 % chance, `.` off.
fn drums(row: &str) -> [Step; MAX_STEPS] {
    let mut s = [Step::OFF; MAX_STEPS];
    for (i, c) in row.chars().take(MAX_STEPS).enumerate() {
        s[i] = match c {
            'X' => Step { on: true, vel: 122, gate: 2, ..Step::OFF },
            'x' => Step { on: true, vel: 96, gate: 2, ..Step::OFF },
            'o' => Step { on: true, vel: 52, gate: 2, ..Step::OFF },
            '?' => Step { on: true, vel: 80, gate: 2, prob: 50, ..Step::OFF },
            _ => Step::OFF,
        };
    }
    s
}

/// Note row: 16 tokens, `.` off, else degree with optional suffixes
/// `_` long (two steps), `-` short, `^` accent, `~` 50 % chance.
fn notes(row: &str) -> [Step; MAX_STEPS] {
    let mut s = [Step::OFF; MAX_STEPS];
    for (i, tok) in row.split_whitespace().take(MAX_STEPS).enumerate() {
        if tok == "." {
            continue;
        }
        let num: String = tok.chars().filter(|c| c.is_ascii_digit() || *c == '-' && tok.starts_with('-')).collect();
        let num = if tok.starts_with('-') { format!("-{}", num.trim_start_matches('-')) } else { num };
        let d: i8 = num.parse().unwrap_or(0);
        let mut st = Step { on: true, note: d, vel: 96, gate: 3, prob: 100 };
        let suf = tok.trim_start_matches('-').trim_start_matches(|c: char| c.is_ascii_digit());
        for c in suf.chars() {
            match c {
                '_' => st.gate = 8,
                '-' => st.gate = 1,
                '^' => st.vel = 120,
                '~' => st.prob = 50,
                _ => {}
            }
        }
        s[i] = st;
    }
    s
}

pub struct Demo {
    pub name: &'static str,
    pub build: fn() -> Pattern,
}

fn base(name: &str, tempo: f32, swing: u8, scale: Scale, root: u8, voices: [&str; 5]) -> Pattern {
    let mut p = Pattern::empty();
    p.name = Name::new(name);
    p.tempo = tempo;
    p.swing = swing;
    p.scale = scale;
    p.root = root;
    for (t, v) in p.tracks.iter_mut().zip(voices) {
        t.voice = Name::new(v);
    }
    p
}

fn night_drive() -> Pattern {
    let mut p = base("Night Drive", 104.0, 54, Scale::Minor, 9, ["kick.deep", "snare.crack", "hat.closed", "bass.round", "lead.pluck"]);
    p.tracks[0].steps = drums("X.......X.x.....");
    p.tracks[1].steps = drums("....X.......X..o");
    p.tracks[2].steps = drums("x.x.X.x.x.x.X.x?");
    p.tracks[3].steps = notes("0 . . 0 . . 3- . 0 . . 0 . 5 4 .");
    p.tracks[4].steps = notes(". . 4 . 7_ . . . 6 . 4 . 2_ . . .");
    p.echo = 3;
    p.tracks[4].send = 4;
    p
}

fn four_floor() -> Pattern {
    let mut p = base("Four Floor", 122.0, 52, Scale::Minor, 2, ["kick.tight", "clap", "hat.open", "bass.acid", "lead.bell"]);
    p.tracks[0].steps = drums("X...X...X...X...");
    p.tracks[1].steps = drums("....X.......X...");
    p.tracks[2].steps = drums("..x...x...x...X.");
    p.tracks[3].steps = notes(". . 0^ . . . 0 3- . . 0^ . . . 0 5-");
    p.tracks[4].steps = notes("0_ . . . . . 4 . . . . . 2_ . . .");
    p.tracks[3].level = 92;
    p
}

fn dusty() -> Pattern {
    let mut p = base("Dusty", 88.0, 62, Scale::Dorian, 4, ["kick.deep", "snare.crack", "hat.closed", "bass.round", "lead.keys"]);
    p.tracks[0].steps = drums("X......x..X.....");
    p.tracks[1].steps = drums("....X..o....X...");
    p.tracks[2].steps = drums("x.x.x.x.x.x.x.xo");
    p.tracks[3].steps = notes("0_ . . . . . . -2 0 . . . 3 . 2 .");
    p.tracks[4].steps = notes("4_ . . . 2 . . . 0_ . . . . . -1~ .");
    p
}

fn electro() -> Pattern {
    let mut p = base("Electro", 112.0, 50, Scale::Phrygian, 5, ["kick.tight", "snare.crack", "hat.closed", "bass.acid", "lead.square"]);
    p.tracks[0].steps = drums("X..x..X...X..x..");
    p.tracks[1].steps = drums("....X.......X...");
    p.tracks[2].steps = drums("xoxoXoxoxoxoXoxo");
    p.tracks[3].steps = notes("0 . 0 1- . 0 . 0 3 . 0 . 1 . 0 .");
    p.tracks[4].steps = notes("7 . . 6 . . 4 . . . 7~ . 8 . 6 .");
    p.tracks[4].level = 76;
    p
}

/// Triplet grid (6 steps per beat, 12 steps = 2 beats), the newer kit.
fn boom_trip() -> Pattern {
    let mut p = base("Boom Trip", 84.0, 50, Scale::PentaMinor, 7, ["kick.boom", "snare.rim", "shaker", "bass.sub", "pad.glass"]);
    p.rate = Rate::Triplet;
    p.length = 12;
    p.tracks[0].steps = drums("X.....x..o..");
    p.tracks[1].steps = drums("......X....o");
    p.tracks[2].steps = drums("x.ox.ox.ox.o");
    p.tracks[3].steps = notes("0_ . . . . . -1 . . 2 . .");
    p.tracks[4].steps = notes("4_ . . . . . . . . . . .");
    p.tracks[4].steps[0].gate = 16;
    p.tracks[4].level = 70;
    p.echo = 4;
    p.echo_time = 1;
    p.tracks[1].send = 5;
    p
}

pub const DEMOS: [Demo; 5] = [
    Demo { name: "Night Drive", build: night_drive },
    Demo { name: "Four Floor", build: four_floor },
    Demo { name: "Dusty", build: dusty },
    Demo { name: "Electro", build: electro },
    Demo { name: "Boom Trip", build: boom_trip },
];
