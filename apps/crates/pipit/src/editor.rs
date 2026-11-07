//! Button-driven editor state (no drawing, no audio, no files): every
//! ZPOD key maps to exactly one action per mode, so it is unit-tested.

use zpod_core::{Key, KeyState};
use zpod_seq::demos::DEMOS;
use zpod_seq::gen;
use zpod_seq::pattern::{Pattern, Scale, Step, MAX_STEPS, TRACKS};
use zpod_voices::Bank;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Grid,
    Step,
    Track,
    Song,
}

impl Mode {
    pub const ALL: [Mode; 4] = [Mode::Grid, Mode::Step, Mode::Track, Mode::Song];
    pub fn name(self) -> &'static str {
        match self {
            Mode::Grid => "GRID",
            Mode::Step => "STEP",
            Mode::Track => "TRACK",
            Mode::Song => "SONG",
        }
    }
    fn next(self) -> Mode {
        Self::ALL[(Self::ALL.iter().position(|&m| m == self).unwrap_or(0) + 1) % 4]
    }
    pub fn params(self) -> &'static [&'static str] {
        match self {
            Mode::Grid => &[],
            Mode::Step => &["Note", "Velocity", "Gate", "Chance"],
            Mode::Track => &["Voice", "Level", "Mute", "Octave"],
            Mode::Song => &["Tempo", "Swing", "Scale", "Key", "Length", "Output"],
        }
    }
}

pub const GATES: [u8; 8] = [1, 2, 3, 4, 6, 8, 12, 16];
pub const OUTPUTS: [(&str, f32); 3] = [("Soft", 0.5), ("Medium", 0.71), ("Full", 1.0)];
pub const SLOTS: u8 = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Save,
    Load,
    Demo,
    Euclid,
    Notes,
    Clear,
    New,
    Undo,
}

impl Action {
    pub const ALL: [Action; 8] = [Action::Save, Action::Load, Action::Demo, Action::Euclid, Action::Notes, Action::Clear, Action::New, Action::Undo];
}

#[derive(Clone, Debug, PartialEq)]
pub struct Menu {
    pub sel: usize,
    pub slot: u8,
    pub demo: usize,
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
}

impl Editor {
    pub fn new(pat: Pattern, bank: Bank) -> Self {
        Editor { pat, undo: None, mode: Mode::Grid, track: 0, step: 0, param: 0, menu: None, help: false, playing: false, output: 0, slot_used: [false; SLOTS as usize], bank }
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
        let ok = matches!(key, Key::Circle | Key::Ok);
        match key {
            Key::Triangle if press => {
                self.mode = self.mode.next();
                self.param = 0;
                Effect::None
            }
            Key::Square if press => {
                self.menu = Some(Menu { sel: 0, slot: 1, demo: 0, hits: 4, seed: self.pat.seed, armed: false });
                Effect::None
            }
            Key::Cross if press => {
                if self.mode == Mode::Grid {
                    self.help = true;
                } else {
                    self.mode = Mode::Grid;
                }
                Effect::None
            }
            _ if ok && press => match self.mode {
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
            Key::Up | Key::Down | Key::Left | Key::Right => self.arrow(key),
            _ => Effect::None,
        }
    }

    fn arrow(&mut self, key: Key) -> Effect {
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
                Effect::None
            }
            Key::Down => {
                self.param = (self.param + 1) % n;
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
                    _ => {
                        if drums {
                            return Effect::Toast("Drums: tune each step (STEP > Note)".into());
                        }
                        self.pat.tracks[tr].octave = (self.pat.tracks[tr].octave as i32 + d).clamp(1, 6) as i8;
                    }
                }
            }
            Mode::Song => match self.param {
                0 => self.pat.tempo = (self.pat.tempo + d as f32).clamp(60.0, 180.0),
                1 => self.pat.swing = (self.pat.swing as i32 + d).clamp(50, 75) as u8,
                2 => {
                    let i = Scale::ALL.iter().position(|&s| s == self.pat.scale).unwrap_or(0) as i32;
                    self.pat.scale = Scale::ALL[(i + d).rem_euclid(Scale::ALL.len() as i32) as usize];
                }
                3 => self.pat.root = (self.pat.root as i32 + d).rem_euclid(12) as u8,
                4 => self.pat.length = (self.pat.length as i32 + d).clamp(1, MAX_STEPS as i32) as u8,
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

#[cfg(test)]
mod tests {
    use super::*;
    use KeyState::*;

    fn ed() -> Editor {
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
        let seen: Vec<Mode> = (0..4).map(|_| { e.key(Key::Triangle, Press); e.mode }).collect();
        assert_eq!(seen, vec![Mode::Step, Mode::Track, Mode::Song, Mode::Grid]);
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
        e.key(Key::Circle, Press);
        assert!(e.pat.tracks[4].mute);
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
        assert_eq!(e.param, 5);
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
        for _ in 0..3 {
            e.key(Key::Down, Press);
        }
        assert_eq!(Action::ALL[e.menu.as_ref().unwrap().sel], Action::Euclid);
        e.key(Key::Right, Press); // 5 hits
        assert_eq!(e.pat, orig, "nothing applied before OK");
        e.key(Key::Cross, Press);
        assert!(e.menu.is_none());
        assert_eq!(e.pat, orig, "cancel applies nothing");
        e.key(Key::Square, Press);
        for _ in 0..3 {
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
}
