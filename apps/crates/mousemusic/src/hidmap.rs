//! USB / Bluetooth mouse + keyboard -> Mouse Music actions (DESIGN.md §3).
//! Input arrives as normalized zpod-input events (begin/update/end); this
//! adapter turns them into musical controls, so the performance code never
//! sees HID.

use zpod_input::{Control, InputEvent, KeyCode, Phase, PointerButton};

/// Mouse counts per grid step (DESIGN.md: 24, adjustable).
pub const COUNTS_PER_STEP: i32 = 24;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// grid steps: +x right, +y up
    Move(i32, i32),
    PlayToggle,
    Help,
    /// close help / overlays
    Back,
    PatternToggle,
    ModeNext,
    /// harmony mode by index into `Mode::ALL`
    Mode(usize),
    VoicingNext,
    Transpose(i32),
    Tempo(i32),
}

#[derive(Default, Debug)]
pub struct HidMap {
    ax: i32,
    ay: i32,
}

impl HidMap {
    pub fn map(&mut self, e: &InputEvent) -> Option<Action> {
        match e.control {
            Control::Pointer => {
                if e.phase == Phase::End {
                    return None; // keep the sub-step remainder for the next gesture
                }
                self.ax += e.dx;
                self.ay -= e.dy; // mouse y grows downwards; grid y grows up
                let sx = self.ax / COUNTS_PER_STEP;
                let sy = self.ay / COUNTS_PER_STEP;
                self.ax -= sx * COUNTS_PER_STEP;
                self.ay -= sy * COUNTS_PER_STEP;
                (sx != 0 || sy != 0).then_some(Action::Move(sx, sy))
            }
            Control::Wheel => (e.dy != 0).then_some(Action::Tempo(4 * e.dy)),
            Control::Button(b) if e.phase == Phase::Begin => match b {
                // left = start/stop (DESIGN.md: hold = latch; latch is not in phase 1)
                PointerButton::Left => Some(Action::PlayToggle),
                PointerButton::Right => Some(Action::Help),
                PointerButton::Middle => Some(Action::PatternToggle),
                _ => None,
            },
            Control::Button(_) => None,
            Control::Key(k) => {
                if e.phase == Phase::End {
                    return None;
                }
                let repeat = e.phase == Phase::Update;
                let a = match k {
                    KeyCode::Left => Action::Move(-1, 0),
                    KeyCode::Right => Action::Move(1, 0),
                    KeyCode::Up => Action::Move(0, 1),
                    KeyCode::Down => Action::Move(0, -1),
                    _ if repeat => return None, // only arrows autorepeat
                    KeyCode::Enter => Action::PatternToggle,
                    KeyCode::Space => Action::PlayToggle,
                    KeyCode::Esc => Action::Back,
                    KeyCode::Tab => Action::ModeNext,
                    KeyCode::Char(_) => match e.text? {
                        '?' | 'h' | 'H' => Action::Help,
                        c @ '1'..='6' => Action::Mode(c as usize - '1' as usize),
                        'v' | 'V' => Action::VoicingNext,
                        'p' | 'P' => Action::PatternToggle,
                        '-' | '_' => Action::Transpose(-1),
                        '=' | '+' => Action::Transpose(1),
                        '[' | '{' => Action::Tempo(-4),
                        ']' | '}' => Action::Tempo(4),
                        _ => return None,
                    },
                    _ => return None,
                };
                Some(a)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zpod_input::event::Mods;

    fn ev(control: Control, phase: Phase, dx: i32, dy: i32, text: Option<char>) -> InputEvent {
        InputEvent { t_us: 0, source: 1, contact: 0, phase, control, dx, dy, text, mods: Mods::default() }
    }

    #[test]
    fn pointer_accumulates_sub_steps() {
        let mut m = HidMap::default();
        assert_eq!(m.map(&ev(Control::Pointer, Phase::Begin, 10, 0, None)), None);
        assert_eq!(m.map(&ev(Control::Pointer, Phase::Update, 20, 0, None)), Some(Action::Move(1, 0)));
        assert_eq!(m.map(&ev(Control::Pointer, Phase::Update, 0, -50, None)), Some(Action::Move(0, 2)), "mouse up = grid up");
        assert_eq!(m.map(&ev(Control::Pointer, Phase::Update, -54, 0, None)), Some(Action::Move(-2, 0)));
        assert_eq!(m.map(&ev(Control::Pointer, Phase::End, 0, 0, None)), None);
    }

    #[test]
    fn keys_and_buttons() {
        let mut m = HidMap::default();
        let k = |c| Control::Key(c);
        assert_eq!(m.map(&ev(k(KeyCode::Up), Phase::Begin, 0, 0, None)), Some(Action::Move(0, 1)));
        assert_eq!(m.map(&ev(k(KeyCode::Up), Phase::Update, 0, 0, None)), Some(Action::Move(0, 1)), "arrows repeat");
        assert_eq!(m.map(&ev(k(KeyCode::Space), Phase::Update, 0, 0, None)), None, "space does not repeat");
        assert_eq!(m.map(&ev(k(KeyCode::Space), Phase::Begin, 0, 0, Some(' '))), Some(Action::PlayToggle));
        assert_eq!(m.map(&ev(k(KeyCode::Char(4)), Phase::Begin, 0, 0, Some('3'))), Some(Action::Mode(2)));
        assert_eq!(m.map(&ev(k(KeyCode::Char(53)), Phase::Begin, 0, 0, Some('?'))), Some(Action::Help));
        assert_eq!(m.map(&ev(k(KeyCode::Char(26)), Phase::Begin, 0, 0, Some('['))), Some(Action::Tempo(-4)));
        assert_eq!(m.map(&ev(k(KeyCode::Char(30)), Phase::Begin, 0, 0, Some('a'))), None);
        assert_eq!(m.map(&ev(k(KeyCode::Esc), Phase::End, 0, 0, None)), None);
        assert_eq!(m.map(&ev(Control::Button(PointerButton::Left), Phase::Begin, 0, 0, None)), Some(Action::PlayToggle));
        assert_eq!(m.map(&ev(Control::Button(PointerButton::Left), Phase::End, 0, 0, None)), None);
        assert_eq!(m.map(&ev(Control::Wheel, Phase::Update, 0, -2, None)), Some(Action::Tempo(-8)));
    }
}
