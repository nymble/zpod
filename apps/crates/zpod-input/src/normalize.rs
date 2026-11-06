//! Raw evdev (type, code, value) triples -> normalized [`InputEvent`]s.
//! Pure state machine: no I/O, so it is tested on any host and shared by
//! the device reader and the simulator.

use crate::event::*;
use crate::keymap::{self, KeyCode};

pub const EV_SYN: u16 = 0x00;
pub const EV_KEY: u16 = 0x01;
pub const EV_REL: u16 = 0x02;
pub const SYN_REPORT: u16 = 0;
pub const SYN_DROPPED: u16 = 3;
pub const REL_X: u16 = 0x00;
pub const REL_Y: u16 = 0x01;
pub const REL_HWHEEL: u16 = 0x06;
pub const REL_WHEEL: u16 = 0x08;
pub const REL_WHEEL_HI_RES: u16 = 0x0b;
pub const REL_HWHEEL_HI_RES: u16 = 0x0c;
pub const BTN_MOUSE: u16 = 0x110;
pub const BTN_LEFT: u16 = 0x110;
pub const BTN_RIGHT: u16 = 0x111;
pub const BTN_MIDDLE: u16 = 0x112;
pub const BTN_SIDE: u16 = 0x113;
pub const BTN_EXTRA: u16 = 0x114;
pub const BTN_TASK: u16 = 0x117;

/// Pointer rest time that ends a pointer contact.
pub const POINTER_IDLE_US: u64 = 250_000;

fn button(code: u16) -> PointerButton {
    match code {
        BTN_LEFT => PointerButton::Left,
        BTN_RIGHT => PointerButton::Right,
        BTN_MIDDLE => PointerButton::Middle,
        BTN_SIDE => PointerButton::Side,
        BTN_EXTRA => PointerButton::Extra,
        c => PointerButton::Other(c),
    }
}

/// Per-device normalizer.
#[derive(Debug)]
pub struct Normalizer {
    pub source: SourceId,
    shift_l: bool,
    shift_r: bool,
    ctrl: u8,
    alt: u8,
    meta: u8,
    caps: bool,
    /// keys / buttons currently down (contact ids), to end them on unplug
    down: Vec<(u32, Control)>,
    dx: i32,
    dy: i32,
    wheel: i32,
    hwheel: i32,
    /// legacy detents in the current report (used only until hi-res is seen)
    lw: i32,
    lhw: i32,
    /// hi-res wheel seen: ignore the legacy detent events (kernels send both)
    hires: bool,
    pointer_active: bool,
    last_motion_us: u64,
    /// SYN_DROPPED: discard until the next SYN_REPORT
    dropping: bool,
}

impl Normalizer {
    pub fn new(source: SourceId) -> Self {
        Normalizer {
            source,
            shift_l: false,
            shift_r: false,
            ctrl: 0,
            alt: 0,
            meta: 0,
            caps: false,
            down: Vec::new(),
            dx: 0,
            dy: 0,
            wheel: 0,
            hwheel: 0,
            lw: 0,
            lhw: 0,
            hires: false,
            pointer_active: false,
            last_motion_us: 0,
            dropping: false,
        }
    }

    pub fn mods(&self) -> Mods {
        Mods { shift: self.shift_l || self.shift_r, ctrl: self.ctrl > 0, alt: self.alt > 0, meta: self.meta > 0 }
    }

    fn ev(&self, t_us: u64, contact: u32, phase: Phase, control: Control) -> InputEvent {
        InputEvent { t_us, source: self.source, contact, phase, control, dx: 0, dy: 0, text: None, mods: self.mods() }
    }

    /// Feed one raw event; normalized events are appended to `out`.
    pub fn feed(&mut self, t_us: u64, ty: u16, code: u16, value: i32, out: &mut Vec<InputEvent>) {
        if self.dropping {
            if ty == EV_SYN && code == SYN_REPORT {
                self.dropping = false;
                self.dx = 0;
                self.dy = 0;
                self.wheel = 0;
                self.hwheel = 0;
                self.lw = 0;
                self.lhw = 0;
            }
            return;
        }
        match ty {
            EV_SYN if code == SYN_DROPPED => self.dropping = true,
            EV_SYN if code == SYN_REPORT => self.flush(t_us, out),
            EV_REL => match code {
                REL_X => self.dx += value,
                REL_Y => self.dy += value,
                REL_WHEEL => self.lw += value,
                REL_HWHEEL => self.lhw += value,
                REL_WHEEL_HI_RES => {
                    self.hires = true;
                    self.wheel += value
                }
                REL_HWHEEL_HI_RES => {
                    self.hires = true;
                    self.hwheel += value
                }
                _ => {}
            },
            EV_KEY => self.key(t_us, code, value, out),
            _ => {}
        }
    }

    fn key(&mut self, t_us: u64, code: u16, value: i32, out: &mut Vec<InputEvent>) {
        if (BTN_MOUSE..=BTN_TASK).contains(&code) {
            let contact = CONTACT_BUTTON_BASE + (code - BTN_MOUSE) as u32;
            let control = Control::Button(button(code));
            match value {
                1 if !self.is_down(contact) => {
                    self.down.push((contact, control));
                    out.push(self.ev(t_us, contact, Phase::Begin, control));
                }
                0 if self.is_down(contact) => {
                    self.down.retain(|d| d.0 != contact);
                    out.push(self.ev(t_us, contact, Phase::End, control));
                }
                _ => {}
            }
            return;
        }
        if code >= 0x100 {
            return; // joystick / tablet / other buttons: not normalized yet
        }
        let k = keymap::key_code(code);
        let contact = code as u32;
        let control = Control::Key(k);
        let pressed = value != 0;
        match code {
            keymap::KEY_LEFTSHIFT => self.shift_l = pressed,
            keymap::KEY_RIGHTSHIFT => self.shift_r = pressed,
            _ => {}
        }
        let adj = |c: &mut u8| {
            if value == 1 {
                *c = c.saturating_add(1)
            } else if value == 0 {
                *c = c.saturating_sub(1)
            }
        };
        match k {
            KeyCode::Ctrl => adj(&mut self.ctrl),
            KeyCode::Alt => adj(&mut self.alt),
            KeyCode::Meta => adj(&mut self.meta),
            KeyCode::CapsLock if value == 1 => self.caps = !self.caps,
            _ => {}
        }
        let phase = match value {
            1 if self.is_down(contact) => return,
            1 => {
                self.down.push((contact, control));
                Phase::Begin
            }
            2 if self.is_down(contact) => Phase::Update,
            0 if self.is_down(contact) => {
                self.down.retain(|d| d.0 != contact);
                Phase::End
            }
            _ => return,
        };
        let mut e = self.ev(t_us, contact, phase, control);
        if phase != Phase::End && !self.mods().ctrl && !self.mods().alt && !self.mods().meta {
            e.text = keymap::key_text(code, self.mods().shift, self.caps);
        }
        out.push(e);
    }

    fn is_down(&self, contact: u32) -> bool {
        self.down.iter().any(|d| d.0 == contact)
    }

    fn flush(&mut self, t_us: u64, out: &mut Vec<InputEvent>) {
        if self.dx != 0 || self.dy != 0 {
            let phase = if self.pointer_active && t_us.saturating_sub(self.last_motion_us) < POINTER_IDLE_US {
                Phase::Update
            } else {
                if self.pointer_active {
                    // rest that tick() did not see yet: close the old contact first
                    out.push(self.ev(self.last_motion_us + POINTER_IDLE_US, CONTACT_POINTER, Phase::End, Control::Pointer));
                }
                Phase::Begin
            };
            let mut e = self.ev(t_us, CONTACT_POINTER, phase, Control::Pointer);
            e.dx = self.dx;
            e.dy = self.dy;
            out.push(e);
            self.pointer_active = true;
            self.last_motion_us = t_us;
            self.dx = 0;
            self.dy = 0;
        }
        // whole detents only (hi-res: 120 per detent). Kernels with hi-res
        // wheels send both kinds: once hi-res is seen, legacy is ignored.
        if !self.hires {
            self.wheel += self.lw * 120;
            self.hwheel += self.lhw * 120;
        }
        self.lw = 0;
        self.lhw = 0;
        if self.wheel.abs() >= 120 || self.hwheel.abs() >= 120 {
            let mut e = self.ev(t_us, CONTACT_WHEEL, Phase::Update, Control::Wheel);
            e.dy = self.wheel / 120;
            e.dx = self.hwheel / 120;
            self.wheel %= 120;
            self.hwheel %= 120;
            out.push(e);
        }
    }

    /// Time passes with no events: end a resting pointer contact.
    pub fn tick(&mut self, now_us: u64, out: &mut Vec<InputEvent>) {
        if self.pointer_active && now_us.saturating_sub(self.last_motion_us) >= POINTER_IDLE_US {
            self.pointer_active = false;
            out.push(self.ev(self.last_motion_us + POINTER_IDLE_US, CONTACT_POINTER, Phase::End, Control::Pointer));
        }
    }

    /// Device gone: end every open contact (keys, buttons, pointer).
    pub fn unplug(&mut self, t_us: u64, out: &mut Vec<InputEvent>) {
        for (contact, control) in std::mem::take(&mut self.down) {
            out.push(self.ev(t_us, contact, Phase::End, control));
        }
        if self.pointer_active {
            self.pointer_active = false;
            out.push(self.ev(t_us, CONTACT_POINTER, Phase::End, Control::Pointer));
        }
        self.shift_l = false;
        self.shift_r = false;
        self.ctrl = 0;
        self.alt = 0;
        self.meta = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(n: &mut Normalizer, evs: &[(u64, u16, u16, i32)]) -> Vec<InputEvent> {
        let mut out = vec![];
        for &(t, ty, c, v) in evs {
            n.feed(t, ty, c, v, &mut out);
        }
        out
    }

    #[test]
    fn key_begin_repeat_end_with_text() {
        let mut n = Normalizer::new(7);
        let out = run(&mut n, &[(10, EV_KEY, 30, 1), (11, EV_SYN, 0, 0), (500, EV_KEY, 30, 2), (600, EV_KEY, 30, 0)]);
        assert_eq!(out.len(), 3);
        assert_eq!((out[0].phase, out[0].control, out[0].text, out[0].contact, out[0].source), (Phase::Begin, Control::Key(KeyCode::Char(30)), Some('a'), 30, 7));
        assert_eq!((out[1].phase, out[1].text), (Phase::Update, Some('a')));
        assert_eq!((out[2].phase, out[2].text), (Phase::End, None));
        assert!(out.windows(2).all(|w| w[0].t_us <= w[1].t_us), "monotonic");
    }

    #[test]
    fn shift_and_ctrl() {
        let mut n = Normalizer::new(1);
        let out = run(&mut n, &[(1, EV_KEY, 42, 1), (2, EV_KEY, 3, 1), (3, EV_KEY, 3, 0), (4, EV_KEY, 42, 0), (5, EV_KEY, 29, 1), (6, EV_KEY, 31, 1)]);
        let texts: Vec<_> = out.iter().filter_map(|e| e.text).collect();
        assert_eq!(texts, vec!['@'], "shift+2 = @, ctrl+s gives no text");
        let last = out.last().unwrap();
        assert!(last.mods.ctrl && last.control == Control::Key(KeyCode::Char(31)));
    }

    #[test]
    fn stray_release_and_double_press_ignored() {
        let mut n = Normalizer::new(1);
        let out = run(&mut n, &[(1, EV_KEY, 28, 0), (2, EV_KEY, 28, 1), (3, EV_KEY, 28, 1)]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].control, Control::Key(KeyCode::Enter));
    }

    #[test]
    fn pointer_contact_lifecycle() {
        let mut n = Normalizer::new(2);
        let mut out = run(&mut n, &[(1000, EV_REL, REL_X, 5), (1000, EV_REL, REL_Y, -2), (1000, EV_SYN, 0, 0), (9000, EV_REL, REL_X, 3), (9000, EV_SYN, 0, 0)]);
        assert_eq!(out.len(), 2);
        assert_eq!((out[0].phase, out[0].dx, out[0].dy), (Phase::Begin, 5, -2));
        assert_eq!((out[1].phase, out[1].dx), (Phase::Update, 3));
        n.tick(100_000, &mut out);
        assert_eq!(out.len(), 2, "not idle yet");
        n.tick(9000 + POINTER_IDLE_US, &mut out);
        assert_eq!(out[2].phase, Phase::End);
        assert_eq!(out[2].control, Control::Pointer);
        // motion after rest = a new contact
        n.feed(2_000_000, EV_REL, REL_X, 1, &mut out);
        n.feed(2_000_000, EV_SYN, 0, 0, &mut out);
        assert_eq!(out[3].phase, Phase::Begin);
    }

    #[test]
    fn late_motion_closes_old_contact_without_tick() {
        let mut n = Normalizer::new(2);
        let out = run(&mut n, &[(0, EV_REL, REL_X, 1), (0, EV_SYN, 0, 0), (1_000_000, EV_REL, REL_X, 1), (1_000_000, EV_SYN, 0, 0)]);
        let phases: Vec<_> = out.iter().map(|e| e.phase).collect();
        assert_eq!(phases, vec![Phase::Begin, Phase::End, Phase::Begin]);
        assert!(out.windows(2).all(|w| w[0].t_us <= w[1].t_us));
    }

    #[test]
    fn buttons_and_wheel() {
        let mut n = Normalizer::new(3);
        let out = run(
            &mut n,
            &[(1, EV_KEY, BTN_LEFT, 1), (1, EV_SYN, 0, 0), (2, EV_REL, REL_WHEEL, 1), (2, EV_REL, REL_WHEEL_HI_RES, 120), (2, EV_SYN, 0, 0), (3, EV_REL, REL_WHEEL, -1), (3, EV_REL, REL_WHEEL_HI_RES, -60), (3, EV_SYN, 0, 0), (4, EV_REL, REL_WHEEL_HI_RES, -60), (4, EV_SYN, 0, 0), (5, EV_KEY, BTN_LEFT, 0)],
        );
        assert_eq!((out[0].phase, out[0].control, out[0].contact), (Phase::Begin, Control::Button(PointerButton::Left), 0x100));
        // legacy +1 and hi-res +120 in one report count once; -60 -60 = one detent
        let wheel: Vec<_> = out.iter().filter(|e| e.control == Control::Wheel).map(|e| e.dy).collect();
        assert_eq!(wheel, vec![1, -1]);
        assert_eq!(out.last().unwrap().phase, Phase::End);
    }

    #[test]
    fn syn_dropped_discards_partial_report() {
        let mut n = Normalizer::new(1);
        let out = run(&mut n, &[(1, EV_REL, REL_X, 50), (1, EV_SYN, SYN_DROPPED, 0), (2, EV_REL, REL_X, 9), (2, EV_SYN, 0, 0), (3, EV_REL, REL_X, 4), (3, EV_SYN, 0, 0)]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].dx, 4);
    }

    #[test]
    fn unplug_ends_everything() {
        let mut n = Normalizer::new(4);
        let mut out = run(&mut n, &[(1, EV_KEY, 42, 1), (1, EV_KEY, 30, 1), (1, EV_KEY, BTN_RIGHT, 1), (2, EV_REL, REL_X, 1), (2, EV_SYN, 0, 0)]);
        let before = out.len();
        n.unplug(10, &mut out);
        let ends: Vec<_> = out[before..].iter().map(|e| (e.phase, e.contact)).collect();
        assert_eq!(ends.len(), 4, "{ends:?}"); // shift, a, right button, pointer
        assert!(ends.iter().all(|e| e.0 == Phase::End));
        assert!(!n.mods().shift);
    }

    #[test]
    fn debug_redacts_text() {
        let mut n = Normalizer::new(1);
        let out = run(&mut n, &[(1, EV_KEY, 25, 1)]); // 'p'
        let s = format!("{:?}", out[0]);
        assert!(!s.contains("'p'") && !s.contains("25") && s.contains("redacted"), "{s}");
    }
}
