//! Button-driven editor state (no drawing, no audio, no files): every
//! ZPOD key maps to exactly one action per mode, so it is unit-tested.

use zpod_core::{Key, KeyState};
use zpod_seq::demos::DEMOS;
use zpod_seq::gen;
use zpod_seq::pattern::{Macros, Pattern, Rate, Scale, Step, DRUM_PITCH_MAX, ECHO_TIMES, FX_MAX, MACRO_MAX, MACRO_MIN, MAX_STEPS, TRACKS, TUNE_CENTS_MAX};
use zpod_voices::Bank;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// performance: tempo, tap, rate, swing, lane mute/solo (first screen)
    Live,
    Grid,
    Step,
    Track,
    Song,
}

impl Mode {
    pub const ALL: [Mode; 5] = [Mode::Live, Mode::Grid, Mode::Step, Mode::Track, Mode::Song];
    pub fn name(self) -> &'static str {
        match self {
            Mode::Live => "LIVE",
            Mode::Grid => "GRID",
            Mode::Step => "STEP",
            Mode::Track => "TRACK",
            Mode::Song => "SONG",
        }
    }
    fn next(self) -> Mode {
        Self::ALL[(Self::ALL.iter().position(|&m| m == self).unwrap_or(0) + 1) % Self::ALL.len()]
    }
    pub fn params(self) -> &'static [&'static str] {
        match self {
            Mode::Live => &LIVE_ROWS,
            Mode::Grid => &[],
            Mode::Step => &["Note", "Velocity", "Gate", "Chance"],
            Mode::Track => &["Voice", "Level", "Mute", "Octave", "Tone", "Decay", "Pitch", "Drive", "Echo send"],
            Mode::Song => &["Tempo", "Swing", "Rate", "Scale", "Key", "Length", "Drive", "Echo", "Echo time", "Output"],
        }
    }
}

/// LIVE rows: 0 tempo (OK = tap), 1 rate, 2 swing, 3.. one per lane.
pub const LIVE_ROWS: [&str; 3 + TRACKS] = ["Tempo", "Rate", "Swing", "KICK", "SNARE", "HAT", "BASS", "LEAD"];
pub const LIVE_LANE0: usize = 3;
pub const TEMPO_UI: (f32, f32) = (60.0, 180.0);
/// taps closer than this are contact bounce / double presses
pub const TAP_DEBOUNCE_MS: u64 = 120;
/// a pause longer than this starts a new tap sequence
pub const TAP_RESET_MS: u64 = 2000;
/// at most this many recent intervals are averaged
pub const TAP_AVG: usize = 4;

/// Drum kits for the KICK/SNARE/HAT lanes (voices; macros reset).
pub const KITS: [(&str, [&str; 3]); 5] = [
    ("Deep", ["kick.deep", "snare.crack", "hat.closed"]),
    ("Tight", ["kick.tight", "clap", "hat.closed"]),
    ("Boom", ["kick.boom", "snare.rim", "shaker"]),
    ("Toms", ["tom.low", "tom.high", "hat.open"]),
    ("Soft", ["kick.deep", "snare.soft", "shaker"]),
];

/// Lane state in LIVE.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lane {
    Mute,
    On,
    Solo,
}

pub const GATES: [u8; 8] = [1, 2, 3, 4, 6, 8, 12, 16];
pub const OUTPUTS: [(&str, f32); 3] = [("Soft", 0.5), ("Medium", 0.71), ("Full", 1.0)];
pub const SLOTS: u8 = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Save,
    Load,
    Demo,
    Kit,
    Euclid,
    Notes,
    Clear,
    New,
    Undo,
}

impl Action {
    pub const ALL: [Action; 9] = [Action::Save, Action::Load, Action::Demo, Action::Kit, Action::Euclid, Action::Notes, Action::Clear, Action::New, Action::Undo];
}

#[derive(Clone, Debug, PartialEq)]
pub struct Menu {
    pub sel: usize,
    pub slot: u8,
    pub demo: usize,
    pub kit: usize,
    pub hits: u8,
    pub seed: u32,
    /// Save onto a used slot: first OK arms, second OK writes
    pub armed: bool,
}

/// What the main loop must do after a key.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    None,
    /// pattern changed (send to audio, autosave later)
    Edited,
    Transport(bool),
    SaveSlot(u8),
    LoadSlot(u8),
    /// play bank voice `index` once on the audition slot with these macros
    Audition { voice: usize, macros: Macros, pitched: bool, octave: i8 },
    Toast(String),
    Exit,
}

pub struct Editor {
    pub pat: Pattern,
    pub undo: Option<Pattern>,
    pub mode: Mode,
    pub track: usize,
    pub step: usize,
    pub param: usize,
    pub menu: Option<Menu>,
    pub help: bool,
    pub playing: bool,
    pub output: usize,
    /// which slots hold a saved pattern (filled in by main)
    pub slot_used: [bool; SLOTS as usize],
    pub bank: Bank,
    /// voice browser for the current track (TRACK > Voice > OK)
    pub browser: Option<Browser>,
    /// set by the caller before `key` (ms since start); used by tap tempo
    pub now_ms: u64,
    taps: [u64; TAP_AVG + 1],
    ntaps: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Browser {
    /// bank indices in browser order
    pub list: Vec<usize>,
    pub sel: usize,
}

impl Editor {
    pub fn new(pat: Pattern, bank: Bank) -> Self {
        Editor { pat, undo: None, mode: Mode::Live, track: 0, step: 0, param: 0, menu: None, help: false, playing: false, output: 0, slot_used: [false; SLOTS as usize], bank, browser: None, now_ms: 0, taps: [0; TAP_AVG + 1], ntaps: 0 }
    }

    pub fn lane(&self, t: usize) -> Lane {
        let tr = &self.pat.tracks[t];
        if tr.mute {
            Lane::Mute
        } else if tr.solo {
            Lane::Solo
        } else {
            Lane::On
        }
    }

    fn set_lane(&mut self, t: usize, l: Lane) {
        let tr = &mut self.pat.tracks[t];
        tr.mute = l == Lane::Mute;
        tr.solo = l == Lane::Solo;
    }

    /// Tap tempo: returns the new tempo once two taps are in.
    pub fn tap(&mut self) -> Option<f32> {
        let now = self.now_ms;
        if self.ntaps > 0 {
            let last = self.taps[self.ntaps - 1];
            let gap = now.saturating_sub(last);
            if gap < TAP_DEBOUNCE_MS {
                return None; // bounce / double press
            }
            if gap > TAP_RESET_MS {
                self.ntaps = 0;
            }
        }
        if self.ntaps == self.taps.len() {
            self.taps.copy_within(1.., 0);
            self.ntaps -= 1;
        }
        self.taps[self.ntaps] = now;
        self.ntaps += 1;
        if self.ntaps < 2 {
            return None;
        }
        let span = (self.taps[self.ntaps - 1] - self.taps[0]) as f32;
        let bpm = 60_000.0 * (self.ntaps - 1) as f32 / span;
        Some(bpm.round().clamp(TEMPO_UI.0, TEMPO_UI.1))
    }

    pub fn taps(&self) -> usize {
        self.ntaps
    }

    pub fn cur(&self) -> &Step {
        &self.pat.tracks[self.track].steps[self.step]
    }

    fn cur_mut(&mut self) -> &mut Step {
        &mut self.pat.tracks[self.track].steps[self.step]
    }

    /// Replace the pattern by an explicit action, keeping Undo.
    pub fn replace(&mut self, p: Pattern) {
        self.undo = Some(self.pat);
        self.pat = p;
    }

    pub fn key(&mut self, key: Key, state: KeyState) -> Effect {
        if state == KeyState::Release || matches!(key, Key::VolUp | Key::VolDown) {
            return Effect::None;
        }
        let press = state == KeyState::Press;
        if key == Key::Home {
            return if press { Effect::Exit } else { Effect::None };
        }
        if key == Key::PlayPause {
            if !press {
                return Effect::None;
            }
            self.playing = !self.playing;
            return Effect::Transport(self.playing);
        }
        if self.help {
            if press {
                self.help = false;
            }
            return Effect::None;
        }
        if self.menu.is_some() {
            return self.menu_key(key, press);
        }
        if self.browser.is_some() {
            return self.browser_key(key, press);
        }
        let ok = matches!(key, Key::Circle | Key::Ok);
        match key {
            Key::Triangle if press => {
                self.mode = self.mode.next();
                self.param = 0;
                Effect::None
            }
            Key::Square if press => {
                self.menu = Some(Menu { sel: 0, slot: 1, demo: 0, kit: 0, hits: 4, seed: self.pat.seed, armed: false });
                Effect::None
            }
            Key::Cross if press => {
                if matches!(self.mode, Mode::Grid | Mode::Live) {
                    self.help = true;
                } else {
                    self.mode = Mode::Grid;
                }
                Effect::None
            }
            _ if ok && press => match self.mode {
                Mode::Live => match self.param {
                    0 => match self.tap() {
                        Some(bpm) => {
                            self.pat.tempo = bpm;
                            Effect::Edited
                        }
                        None if self.ntaps == 1 => Effect::Toast("Tap: keep tapping the beat".into()),
                        None => Effect::None,
                    },
                    p if p >= LIVE_LANE0 => {
                        let t = p - LIVE_LANE0;
                        let l = if self.lane(t) == Lane::Mute { Lane::On } else { Lane::Mute };
                        self.set_lane(t, l);
                        Effect::Edited
                    }
                    _ => Effect::None,
                },
                Mode::Track if self.param == 0 => self.open_browser(),
                Mode::Grid | Mode::Step => {
                    let s = self.cur_mut();
                    s.on = !s.on;
                    Effect::Edited
                }
                Mode::Track => {
                    let t = &mut self.pat.tracks[self.track];
                    t.mute = !t.mute;
                    Effect::Edited
                }
                Mode::Song => Effect::None,
            },
            Key::Up | Key::Down | Key::Left | Key::Right => self.arrow(key, press),
            _ => Effect::None,
        }
    }

    fn open_browser(&mut self) -> Effect {
        let drums = !self.pat.tracks[self.track].pitched;
        let list = self.bank.browse(drums);
        if list.is_empty() {
            return Effect::Toast("No voices for this track".into());
        }
        let cur = self.bank.find(self.pat.tracks[self.track].voice.as_str());
        let sel = cur.and_then(|c| list.iter().position(|&i| i == c)).unwrap_or(0);
        self.browser = Some(Browser { list, sel });
        Effect::None
    }

    fn browser_key(&mut self, key: Key, press: bool) -> Effect {
        let b = self.browser.as_mut().unwrap();
        let n = b.list.len();
        match key {
            Key::Up => {
                b.sel = (b.sel + n - 1) % n;
                Effect::None
            }
            Key::Down => {
                b.sel = (b.sel + 1) % n;
                Effect::None
            }
            // audition is explicit: Right (press only, never on repeat)
            Key::Right if press => {
                let tr = &self.pat.tracks[self.track];
                Effect::Audition { voice: b.list[b.sel], macros: tr.macros, pitched: tr.pitched, octave: tr.octave }
            }
            Key::Circle | Key::Ok if press => {
                let v = self.bank.voices[b.list[b.sel]].id;
                self.browser = None;
                if self.pat.tracks[self.track].voice == v {
                    return Effect::None;
                }
                self.pat.tracks[self.track].voice = v;
                Effect::Edited
            }
            Key::Cross | Key::Square | Key::Left if press => {
                self.browser = None;
                Effect::None
            }
            _ => Effect::None,
        }
    }

    fn arrow(&mut self, key: Key, press: bool) -> Effect {
        if self.mode == Mode::Live && matches!(key, Key::Left | Key::Right) && self.param >= LIVE_LANE0 {
            // lane state: Left mutes, Right solos (from "on"); held keys do nothing
            if !press {
                return Effect::None;
            }
            let t = self.param - LIVE_LANE0;
            let l = match (self.lane(t), key) {
                (Lane::On, Key::Left) => Lane::Mute,
                (Lane::On, _) => Lane::Solo,
                (Lane::Solo, Key::Left) | (Lane::Mute, Key::Right) => Lane::On,
                (l, _) => l,
            };
            if l == self.lane(t) {
                return Effect::None;
            }
            self.set_lane(t, l);
            return Effect::Edited;
        }
        if self.mode == Mode::Grid {
            match key {
                Key::Up => self.track = (self.track + TRACKS - 1) % TRACKS,
                Key::Down => self.track = (self.track + 1) % TRACKS,
                Key::Left => self.step = (self.step + MAX_STEPS - 1) % MAX_STEPS,
                _ => self.step = (self.step + 1) % MAX_STEPS,
            }
            return Effect::None;
        }
        let n = self.mode.params().len();
        match key {
            Key::Up => {
                self.param = (self.param + n - 1) % n;
                if self.mode == Mode::Live && self.param >= LIVE_LANE0 {
                    self.track = self.param - LIVE_LANE0;
                }
                Effect::None
            }
            Key::Down => {
                self.param = (self.param + 1) % n;
                if self.mode == Mode::Live && self.param >= LIVE_LANE0 {
                    self.track = self.param - LIVE_LANE0;
                }
                Effect::None
            }
            _ => {
                let d = if key == Key::Right { 1 } else { -1 };
                self.adjust(d)
            }
        }
    }

    fn adjust(&mut self, d: i32) -> Effect {
        let tr = self.track;
        match self.mode {
            Mode::Grid => return Effect::None,
            Mode::Live => match self.param {
                0 => self.pat.tempo = (self.pat.tempo.round() + d as f32).clamp(TEMPO_UI.0, TEMPO_UI.1),
                1 => self.pat.rate = step_rate(self.pat.rate, d),
                2 => self.pat.swing = (self.pat.swing as i32 + d).clamp(50, 75) as u8,
                _ => return Effect::None,
            },
            Mode::Step => {
                let param = self.param;
                let s = self.cur_mut();
                match param {
                    0 => s.note = (s.note as i32 + d).clamp(Step::NOTE_MIN as i32, Step::NOTE_MAX as i32) as i8,
                    1 => s.vel = (s.vel as i32 + 8 * d).clamp(8, 127) as u8,
                    2 => {
                        let i = GATES.iter().position(|&g| g >= s.gate).unwrap_or(0) as i32;
                        s.gate = GATES[(i + d).clamp(0, GATES.len() as i32 - 1) as usize];
                    }
                    _ => s.prob = (s.prob as i32 + 10 * d).clamp(0, 100) as u8,
                }
            }
            Mode::Track => {
                let drums = !self.pat.tracks[tr].pitched;
                match self.param {
                    0 => {
                        if let Some(i) = self.bank.cycle(self.pat.tracks[tr].voice.as_str(), d, drums) {
                            self.pat.tracks[tr].voice = self.bank.voices[i].id;
                        }
                    }
                    1 => self.pat.tracks[tr].level = (self.pat.tracks[tr].level as i32 + 8 * d).clamp(0, 127) as u8,
                    2 => self.pat.tracks[tr].mute = d > 0,
                    3 => {
                        if drums {
                            return Effect::Toast("Drums: use Pitch (semitones) or STEP > Note".into());
                        }
                        self.pat.tracks[tr].octave = (self.pat.tracks[tr].octave as i32 + d).clamp(1, 6) as i8;
                    }
                    4 => {
                        let m = &mut self.pat.tracks[tr].macros;
                        m.tone = (m.tone as i32 + d).clamp(MACRO_MIN as i32, MACRO_MAX as i32) as i8;
                    }
                    5 => {
                        let m = &mut self.pat.tracks[tr].macros;
                        m.decay = (m.decay as i32 + d).clamp(MACRO_MIN as i32, MACRO_MAX as i32) as i8;
                    }
                    6 => {
                        // drums: semitones; tuned: cents in steps of 5
                        let (step, max) = if drums { (1, DRUM_PITCH_MAX) } else { (5, TUNE_CENTS_MAX) };
                        let m = &mut self.pat.tracks[tr].macros;
                        m.pitch = (m.pitch as i32 + step * d).clamp(-(max as i32), max as i32) as i8;
                    }
                    7 => {
                        let m = &mut self.pat.tracks[tr].macros;
                        m.drive = (m.drive as i32 + d).clamp(0, MACRO_MAX as i32) as i8;
                    }
                    _ => self.pat.tracks[tr].send = (self.pat.tracks[tr].send as i32 + d).clamp(0, FX_MAX as i32) as u8,
                }
            }
            Mode::Song => match self.param {
                0 => self.pat.tempo = (self.pat.tempo.round() + d as f32).clamp(TEMPO_UI.0, TEMPO_UI.1),
                1 => self.pat.swing = (self.pat.swing as i32 + d).clamp(50, 75) as u8,
                2 => self.pat.rate = step_rate(self.pat.rate, d),
                3 => {
                    let i = Scale::ALL.iter().position(|&s| s == self.pat.scale).unwrap_or(0) as i32;
                    self.pat.scale = Scale::ALL[(i + d).rem_euclid(Scale::ALL.len() as i32) as usize];
                }
                4 => self.pat.root = (self.pat.root as i32 + d).rem_euclid(12) as u8,
                5 => self.pat.length = (self.pat.length as i32 + d).clamp(1, MAX_STEPS as i32) as u8,
                6 => self.pat.drive = (self.pat.drive as i32 + d).clamp(0, FX_MAX as i32) as u8,
                7 => self.pat.echo = (self.pat.echo as i32 + d).clamp(0, FX_MAX as i32) as u8,
                8 => self.pat.echo_time = (self.pat.echo_time as i32 + d).clamp(0, ECHO_TIMES.len() as i32 - 1) as u8,
                _ => {
                    self.output = (self.output as i32 + d).clamp(0, OUTPUTS.len() as i32 - 1) as usize;
                    return Effect::Edited;
                }
            },
        }
        Effect::Edited
    }

    fn menu_key(&mut self, key: Key, press: bool) -> Effect {
        let m = self.menu.as_mut().unwrap();
        let n = Action::ALL.len();
        match key {
            Key::Cross | Key::Square if press => {
                self.menu = None;
                Effect::None
            }
            Key::Up => {
                m.sel = (m.sel + n - 1) % n;
                m.armed = false;
                Effect::None
            }
            Key::Down => {
                m.sel = (m.sel + 1) % n;
                m.armed = false;
                Effect::None
            }
            Key::Left | Key::Right => {
                let d: i32 = if key == Key::Right { 1 } else { -1 };
                m.armed = false;
                match Action::ALL[m.sel] {
                    Action::Save | Action::Load => m.slot = ((m.slot as i32 - 1 + d).rem_euclid(SLOTS as i32) + 1) as u8,
                    Action::Demo => m.demo = (m.demo as i32 + d).rem_euclid(DEMOS.len() as i32) as usize,
                    Action::Kit => m.kit = (m.kit as i32 + d).rem_euclid(KITS.len() as i32) as usize,
                    Action::Euclid => m.hits = (m.hits as i32 + d).clamp(1, MAX_STEPS as i32) as u8,
                    Action::Notes => m.seed = (m.seed as i32 + d).clamp(1, 9999) as u32,
                    _ => {}
                }
                Effect::None
            }
            Key::Circle | Key::Ok if press => {
                let m = m.clone();
                let t = self.track;
                let close = |e: &mut Editor| e.menu = None;
                match Action::ALL[m.sel] {
                    Action::Save => {
                        if self.slot_used[m.slot as usize - 1] && !m.armed {
                            self.menu.as_mut().unwrap().armed = true;
                            return Effect::Toast(format!("Slot {} is used: OK again to overwrite", m.slot));
                        }
                        close(self);
                        Effect::SaveSlot(m.slot)
                    }
                    Action::Load => {
                        if !self.slot_used[m.slot as usize - 1] {
                            return Effect::Toast(format!("Slot {} is empty", m.slot));
                        }
                        close(self);
                        Effect::LoadSlot(m.slot)
                    }
                    Action::Demo => {
                        let mut p = (DEMOS[m.demo].build)();
                        p.clamp();
                        self.replace(p);
                        close(self);
                        Effect::Edited
                    }
                    Action::Kit => {
                        let mut p = self.pat;
                        for (t, v) in KITS[m.kit].1.iter().enumerate() {
                            if self.bank.find(v).is_some() {
                                p.tracks[t].voice = zpod_seq::pattern::Name::new(v);
                                p.tracks[t].macros = Macros::default();
                            }
                        }
                        self.replace(p);
                        close(self);
                        Effect::Edited
                    }
                    Action::Euclid => {
                        let p = gen::euclid_fill(&self.pat, t, m.hits, 0);
                        self.replace(p);
                        close(self);
                        Effect::Edited
                    }
                    Action::Notes => {
                        let p = gen::walk_notes(&self.pat, t, m.seed);
                        self.replace(p);
                        close(self);
                        Effect::Edited
                    }
                    Action::Clear => {
                        let p = gen::clear_track(&self.pat, t);
                        self.replace(p);
                        close(self);
                        Effect::Edited
                    }
                    Action::New => {
                        let mut p = Pattern::empty();
                        p.tempo = self.pat.tempo;
                        self.replace(p);
                        close(self);
                        Effect::Edited
                    }
                    Action::Undo => match self.undo.take() {
                        Some(u) => {
                            self.undo = Some(self.pat);
                            self.pat = u;
                            close(self);
                            Effect::Edited
                        }
                        None => Effect::Toast("Nothing to undo".into()),
                    },
                }
            }
            _ => Effect::None,
        }
    }
}

/// Next/previous rate, clamped (no wrap: ½× is the slow end).
fn step_rate(r: Rate, d: i32) -> Rate {
    let i = Rate::ALL.iter().position(|&x| x == r).unwrap_or(1) as i32;
    Rate::ALL[(i + d).clamp(0, Rate::ALL.len() as i32 - 1) as usize]
}

#[cfg(test)]
mod tests {
    use super::*;
    use KeyState::*;

    fn ed() -> Editor {
        let mut e = Editor::new((DEMOS[0].build)(), Bank::builtin());
        e.mode = Mode::Grid; // most tests start from the grid
        e
    }

    fn live() -> Editor {
        Editor::new((DEMOS[0].build)(), Bank::builtin())
    }

    #[test]
    fn grid_navigation_wraps_and_repeats() {
        let mut e = ed();
        e.key(Key::Left, Press);
        assert_eq!(e.step, 15);
        for _ in 0..17 {
            e.key(Key::Right, Repeat);
        }
        assert_eq!(e.step, 0);
        e.key(Key::Up, Press);
        assert_eq!(e.track, 4);
        e.key(Key::Down, Repeat);
        e.key(Key::Down, Repeat);
        assert_eq!(e.track, 1);
        assert_eq!(e.key(Key::Right, Release), Effect::None);
    }

    #[test]
    fn ok_toggles_step_but_not_on_repeat() {
        let mut e = ed();
        let was = e.cur().on;
        assert_eq!(e.key(Key::Circle, Press), Effect::Edited);
        assert_eq!(e.cur().on, !was);
        assert_eq!(e.key(Key::Circle, Repeat), Effect::None);
        assert_eq!(e.cur().on, !was);
        e.key(Key::Ok, Press);
        assert_eq!(e.cur().on, was);
    }

    #[test]
    fn play_toggles_transport_in_any_mode_and_menu() {
        let mut e = ed();
        assert_eq!(e.key(Key::PlayPause, Press), Effect::Transport(true));
        e.key(Key::Square, Press);
        assert!(e.menu.is_some());
        assert_eq!(e.key(Key::PlayPause, Press), Effect::Transport(false));
        assert_eq!(e.key(Key::PlayPause, Repeat), Effect::None);
    }

    #[test]
    fn modes_cycle_and_x_returns_to_grid_then_help() {
        let mut e = ed();
        let seen: Vec<Mode> = (0..5).map(|_| { e.key(Key::Triangle, Press); e.mode }).collect();
        assert_eq!(seen, vec![Mode::Step, Mode::Track, Mode::Song, Mode::Live, Mode::Grid]);
        e.key(Key::Triangle, Press);
        e.key(Key::Cross, Press);
        assert_eq!(e.mode, Mode::Grid);
        assert!(!e.help);
        e.key(Key::Cross, Press);
        assert!(e.help);
        e.key(Key::Left, Press); // any key closes help, does nothing else
        assert!(!e.help);
        assert_eq!(e.step, 0);
    }

    #[test]
    fn step_params_bounded() {
        let mut e = ed();
        e.track = 3;
        e.key(Key::Triangle, Press); // STEP
        for _ in 0..100 {
            e.key(Key::Right, Repeat);
        }
        assert_eq!(e.cur().note, Step::NOTE_MAX);
        e.key(Key::Down, Press); // velocity
        for _ in 0..40 {
            e.key(Key::Left, Repeat);
        }
        assert_eq!(e.cur().vel, 8);
        e.key(Key::Down, Press); // gate
        for _ in 0..20 {
            e.key(Key::Right, Repeat);
        }
        assert_eq!(e.cur().gate, 16);
        e.key(Key::Down, Press); // chance
        for _ in 0..20 {
            e.key(Key::Left, Repeat);
        }
        assert_eq!(e.cur().prob, 0);
        e.key(Key::Down, Press);
        assert_eq!(e.param, 0, "wraps to Note");
        let mut p = e.pat;
        p.clamp();
        assert_eq!(p, e.pat, "always in range");
    }

    #[test]
    fn track_voice_stays_in_family() {
        let mut e = ed();
        e.key(Key::Triangle, Press);
        e.key(Key::Triangle, Press); // TRACK, track 0 (kick)
        for _ in 0..30 {
            e.key(Key::Right, Repeat);
            let id = e.pat.tracks[0].voice.as_str();
            assert!(e.bank.voices[e.bank.find(id).unwrap()].kind.is_drum(), "{id}");
        }
        e.track = 4;
        for _ in 0..30 {
            e.key(Key::Left, Repeat);
            let id = e.pat.tracks[4].voice.as_str();
            assert!(!e.bank.voices[e.bank.find(id).unwrap()].kind.is_drum(), "{id}");
        }
        e.key(Key::Down, Press); // Level
        e.key(Key::Circle, Press);
        assert!(e.pat.tracks[4].mute, "OK on other rows still mutes");
        e.key(Key::Up, Press); // Voice
        assert_eq!(e.key(Key::Circle, Press), Effect::None);
        assert!(e.browser.is_some(), "OK on Voice opens the browser");
    }

    #[test]
    fn song_params_bounded() {
        let mut e = ed();
        for _ in 0..3 {
            e.key(Key::Triangle, Press);
        }
        assert_eq!(e.mode, Mode::Song);
        for _ in 0..300 {
            e.key(Key::Right, Repeat);
        }
        assert_eq!(e.pat.tempo, 180.0);
        e.key(Key::Down, Press);
        for _ in 0..100 {
            e.key(Key::Left, Repeat);
        }
        assert_eq!(e.pat.swing, 50);
        e.key(Key::Up, Press);
        e.key(Key::Up, Press);
        assert_eq!(e.param, 9);
        for _ in 0..9 {
            e.key(Key::Right, Repeat);
        }
        assert_eq!(e.output, 2);
    }

    #[test]
    fn generators_need_explicit_ok_and_undo_restores() {
        let mut e = ed();
        let orig = e.pat;
        e.track = 2;
        e.key(Key::Square, Press);
        for _ in 0..4 {
            e.key(Key::Down, Press);
        }
        assert_eq!(Action::ALL[e.menu.as_ref().unwrap().sel], Action::Euclid);
        e.key(Key::Right, Press); // 5 hits
        assert_eq!(e.pat, orig, "nothing applied before OK");
        e.key(Key::Cross, Press);
        assert!(e.menu.is_none());
        assert_eq!(e.pat, orig, "cancel applies nothing");
        e.key(Key::Square, Press);
        for _ in 0..4 {
            e.key(Key::Down, Press);
        }
        e.key(Key::Right, Press);
        assert_eq!(e.key(Key::Circle, Press), Effect::Edited);
        assert_eq!(e.pat.tracks[2].steps.iter().filter(|s| s.on).count(), 5);
        e.key(Key::Square, Press);
        e.key(Key::Up, Press); // Undo is last
        assert_eq!(Action::ALL[e.menu.as_ref().unwrap().sel], Action::Undo);
        e.key(Key::Circle, Press);
        assert_eq!(e.pat, orig);
    }

    #[test]
    fn save_over_used_slot_needs_second_ok() {
        let mut e = ed();
        e.slot_used[0] = true;
        e.key(Key::Square, Press);
        assert!(matches!(e.key(Key::Circle, Press), Effect::Toast(_)));
        assert!(e.menu.as_ref().unwrap().armed);
        assert_eq!(e.key(Key::Circle, Press), Effect::SaveSlot(1));
        e.key(Key::Square, Press);
        e.key(Key::Down, Press); // Load
        e.key(Key::Right, Press); // slot 2: empty
        assert!(matches!(e.key(Key::Circle, Press), Effect::Toast(_)));
        e.key(Key::Left, Press);
        assert_eq!(e.key(Key::Circle, Press), Effect::LoadSlot(1));
    }

    #[test]
    fn home_exits() {
        let mut e = ed();
        e.key(Key::Square, Press);
        assert_eq!(e.key(Key::Home, Press), Effect::Exit);
    }

    #[test]
    fn opens_in_live_with_tempo_selected() {
        let e = live();
        assert_eq!(e.mode, Mode::Live);
        assert_eq!(e.param, 0);
        assert!(!e.playing, "never starts playing by itself");
    }

    #[test]
    fn live_tempo_rate_swing_bounded() {
        let mut e = live();
        for _ in 0..300 {
            e.key(Key::Right, Repeat);
        }
        assert_eq!(e.pat.tempo, 180.0);
        for _ in 0..300 {
            e.key(Key::Left, Repeat);
        }
        assert_eq!(e.pat.tempo, 60.0);
        e.key(Key::Down, Press); // rate
        for _ in 0..9 {
            e.key(Key::Right, Repeat);
        }
        assert_eq!(e.pat.rate, Rate::Triplet, "clamped at the last rate");
        for _ in 0..9 {
            e.key(Key::Left, Repeat);
        }
        assert_eq!(e.pat.rate, Rate::Half);
        e.key(Key::Down, Press); // swing
        for _ in 0..40 {
            e.key(Key::Right, Repeat);
        }
        assert_eq!(e.pat.swing, 75);
        let mut p = e.pat;
        p.clamp();
        assert_eq!(p, e.pat);
    }

    fn tap_at(e: &mut Editor, ms: u64) -> Effect {
        e.now_ms = ms;
        e.key(Key::Ok, Press)
    }

    #[test]
    fn tap_tempo_averages_debounces_and_resets() {
        let mut e = live();
        let t0 = 10_000;
        assert!(matches!(tap_at(&mut e, t0), Effect::Toast(_)), "first tap only arms");
        assert_eq!(e.pat.tempo, 104.0);
        assert_eq!(tap_at(&mut e, t0 + 500), Effect::Edited);
        assert_eq!(e.pat.tempo, 120.0);
        // a bounce 40 ms later is ignored
        assert_eq!(tap_at(&mut e, t0 + 540), Effect::None);
        assert_eq!(tap_at(&mut e, t0 + 1000), Effect::Edited);
        assert_eq!(tap_at(&mut e, t0 + 1520), Effect::Edited); // slightly late tap
        assert_eq!(e.pat.tempo, 118.0, "average of 3 intervals: 1520/3 ms");
        for k in 1..=10 {
            tap_at(&mut e, t0 + 1520 + k * 400);
        }
        assert_eq!(e.pat.tempo, 150.0, "only recent taps count");
        // a long pause starts over: the next tap only arms
        assert!(matches!(tap_at(&mut e, t0 + 20_000), Effect::Toast(_)));
        assert_eq!(e.pat.tempo, 150.0);
        tap_at(&mut e, t0 + 20_250); // 240 bpm, clamped
        assert_eq!(e.pat.tempo, 180.0);
        // repeat events never tap
        e.now_ms = t0 + 30_000;
        assert_eq!(e.key(Key::Ok, Repeat), Effect::None);
        assert!(e.pat.tracks.iter().all(|t| !t.mute), "tapping never mutes a lane");
    }

    #[test]
    fn live_lanes_mute_solo_without_surprises() {
        let mut e = live();
        for _ in 0..LIVE_LANE0 {
            e.key(Key::Down, Press);
        }
        assert_eq!(e.param, LIVE_LANE0);
        assert_eq!(e.track, 0);
        assert_eq!(e.key(Key::Left, Press), Effect::Edited);
        assert_eq!(e.lane(0), Lane::Mute);
        assert_eq!(e.key(Key::Left, Press), Effect::None, "already muted");
        assert_eq!(e.key(Key::Right, Repeat), Effect::None, "held keys do not change lanes");
        e.key(Key::Right, Press);
        assert_eq!(e.lane(0), Lane::On);
        e.key(Key::Right, Press);
        assert_eq!(e.lane(0), Lane::Solo);
        assert!(e.pat.audible(0) && !e.pat.audible(1));
        e.key(Key::Ok, Press); // OK on a lane: mute toggle (clears solo)
        assert_eq!(e.lane(0), Lane::Mute);
        assert!(!e.pat.tracks[0].solo);
        e.key(Key::Ok, Press);
        assert_eq!(e.lane(0), Lane::On);
        e.key(Key::Down, Press);
        assert_eq!(e.track, 1, "cursor lane follows the LIVE row");
        // wrap back to tempo, OK there is tap, never a mute
        for _ in 0..(LIVE_ROWS.len() - LIVE_LANE0 - 1) {
            e.key(Key::Down, Press);
        }
        assert_eq!(e.param, 0);
        e.now_ms = 1;
        e.key(Key::Ok, Press);
        assert!(e.pat.tracks.iter().all(|t| !t.mute && !t.solo));
    }

    #[test]
    fn browser_auditions_only_on_request_and_chooses_or_cancels() {
        let mut e = ed();
        e.mode = Mode::Track;
        e.track = 3; // bass
        e.key(Key::Ok, Press);
        let b = e.browser.clone().unwrap();
        assert!(b.list.iter().all(|&i| !e.bank.voices[i].kind.is_drum()));
        assert_eq!(e.bank.voices[b.list[b.sel]].id, e.pat.tracks[3].voice, "starts on the current voice");
        let before = e.pat;
        assert_eq!(e.key(Key::Down, Press), Effect::None, "moving never plays");
        assert_eq!(e.key(Key::Down, Repeat), Effect::None);
        assert!(matches!(e.key(Key::Right, Press), Effect::Audition { pitched: true, .. }));
        assert_eq!(e.key(Key::Right, Repeat), Effect::None);
        assert_eq!(e.pat, before, "audition changes nothing");
        assert_eq!(e.key(Key::PlayPause, Press), Effect::Transport(true), "Play still works");
        e.key(Key::Cross, Press);
        assert!(e.browser.is_none());
        assert_eq!(e.pat, before, "cancel keeps the voice");
        e.key(Key::Ok, Press);
        e.key(Key::Down, Press);
        let pick = e.browser.as_ref().map(|b| b.list[b.sel]).unwrap();
        assert_eq!(e.key(Key::Ok, Press), Effect::Edited);
        assert_eq!(e.pat.tracks[3].voice, e.bank.voices[pick].id);
    }

    #[test]
    fn track_macros_bounded_and_pitch_units_by_track_type() {
        let mut e = ed();
        e.mode = Mode::Track;
        for t in [0usize, 4] {
            e.track = t;
            for p in 4..=8 {
                e.param = p;
                for _ in 0..60 {
                    e.key(Key::Right, Repeat);
                }
            }
            let tr = e.pat.tracks[t];
            assert_eq!((tr.macros.tone, tr.macros.decay, tr.macros.drive, tr.send), (10, 10, 10, 10));
            assert_eq!(tr.macros.pitch, if t == 0 { DRUM_PITCH_MAX } else { TUNE_CENTS_MAX });
            let mut p = e.pat;
            p.clamp();
            assert_eq!(p, e.pat);
            assert!(Pattern::from_toml(&e.pat.to_toml()).is_ok());
        }
    }

    #[test]
    fn kit_action_changes_drum_voices_and_undo_restores() {
        let mut e = ed();
        let orig = e.pat;
        e.key(Key::Square, Press);
        for _ in 0..3 {
            e.key(Key::Down, Press);
        }
        assert_eq!(Action::ALL[e.menu.as_ref().unwrap().sel], Action::Kit);
        e.key(Key::Right, Press);
        e.key(Key::Right, Press); // Boom
        assert_eq!(e.key(Key::Ok, Press), Effect::Edited);
        assert_eq!(e.pat.tracks[0].voice.as_str(), "kick.boom");
        assert_eq!(e.pat.tracks[3], orig.tracks[3], "bass untouched");
        for (_, kit) in KITS {
            for v in kit {
                assert!(e.bank.find(v).is_some(), "{v}");
            }
        }
        e.key(Key::Square, Press);
        e.key(Key::Up, Press);
        e.key(Key::Ok, Press);
        assert_eq!(e.pat, orig);
    }
}
