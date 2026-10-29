//! Clock app screens beyond the faces (M5): a menu, the alarm settings, the
//! alarm time editor and the countdown timer. Pure state machine + drawing;
//! the launcher does the ringing (zpod_hal::alarm).
//!
//! Buttons (docs/ui/BUTTONS-AND-MENUS.md):
//! * Face: Up/Down = face, Right/OK/Circle = Clock menu, Triangle = 12/24 h,
//!   Square = seconds on/off, Left/X = exit to the launcher.
//! * Menu / Alarm lists: Up/Down move, Right/OK/Circle = enter or change the
//!   value, Left/X = back.
//! * Time editors (alarm time, timer length): Up/Down change the field,
//!   Right = next field, Left = previous field (on the first field = back),
//!   OK/Circle = save (alarm) or start (timer), X = cancel.
//! * Timer running: OK/Circle = stop it, Left/X = back (it keeps running).
//! * Home, Play/Pause and the volume keys stay global.

use clock_core::{Settings, FACES};
use zpod_core::{DateTime, Key, KeyState};
use zpod_hal::alarm;
use zpod_ui::{
    rect,
    text::{self, Align},
    theme::*,
    widgets::{self, draw_list, ListState, Row, HINTS_H},
    Canvas, Font,
};

pub const MENU: [&str; 5] = ["Alarm", "Timer", "Face", "Seconds", "12/24 hour"];
pub const ALARM_ROWS: [&str; 5] = ["Alarm", "Time", "Repeat", "Snooze", "Test: ring in 5 s"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Screen {
    Face,
    Menu(ListState),
    Alarm(ListState),
    /// Alarm time editor: (hour, minute, field 0/1).
    AlarmTime(u8, u8, usize),
    /// Timer: (minutes, seconds, field 0/1) of the length being set.
    Timer(u32, u32, usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    None,
    Redraw,
    Exit,
    /// Face settings changed (save clock.conf, repaint the face).
    SaveClock,
    /// Alarm/timer settings changed (save alarm.conf).
    SaveAlarm,
    /// Repaint the face from scratch (back from a menu).
    ToFace,
    PlayPause,
}

pub struct Ui {
    pub screen: Screen,
    pub toast: Option<(String, u32)>,
}

impl Default for Ui {
    fn default() -> Self {
        Ui { screen: Screen::Face, toast: None }
    }
}

fn wrap(v: i64, lo: i64, hi: i64) -> i64 {
    lo + (v - lo).rem_euclid(hi - lo + 1)
}

impl Ui {
    /// Handle a key. `now_unix` is used to start the timer / test ring.
    pub fn key(&mut self, key: Key, state: KeyState, s: &mut Settings, a: &mut alarm::Settings, now_unix: i64) -> Action {
        let rep = state == KeyState::Repeat;
        if state == KeyState::Release || (rep && !matches!(key, Key::Up | Key::Down)) {
            return Action::None;
        }
        match key {
            Key::Home => return Action::Exit,
            Key::PlayPause => return Action::PlayPause,
            Key::VolUp | Key::VolDown => return Action::None,
            _ => {}
        }
        let screen = self.screen.clone();
        match screen {
            Screen::Face => match key {
                Key::Cross | Key::Left => Action::Exit,
                Key::Down => {
                    s.face = FACES[(s.face.index() + 1) % FACES.len()];
                    Action::SaveClock
                }
                Key::Up => {
                    s.face = FACES[(s.face.index() + FACES.len() - 1) % FACES.len()];
                    Action::SaveClock
                }
                Key::Right | Key::Ok | Key::Circle => {
                    self.screen = Screen::Menu(ListState::default());
                    Action::Redraw
                }
                Key::Triangle => {
                    s.h24 = !s.h24;
                    Action::SaveClock
                }
                Key::Square => {
                    s.seconds = !s.seconds;
                    Action::SaveClock
                }
                _ => Action::None,
            },
            Screen::Menu(mut ls) => match key {
                Key::Up | Key::Down => {
                    ls.step(if key == Key::Up { -1 } else { 1 }, MENU.len(), MENU.len());
                    self.screen = Screen::Menu(ls);
                    Action::Redraw
                }
                Key::Left | Key::Cross => {
                    self.screen = Screen::Face;
                    Action::ToFace
                }
                Key::Right | Key::Ok | Key::Circle => match MENU[ls.sel] {
                    "Alarm" => {
                        self.screen = Screen::Alarm(ListState::default());
                        Action::Redraw
                    }
                    "Timer" => {
                        let t = a.timer_secs.max(1);
                        self.screen = Screen::Timer(t / 60, t % 60, 0);
                        Action::Redraw
                    }
                    "Face" => {
                        s.face = FACES[(s.face.index() + 1) % FACES.len()];
                        Action::SaveClock
                    }
                    "Seconds" => {
                        s.seconds = !s.seconds;
                        Action::SaveClock
                    }
                    _ => {
                        s.h24 = !s.h24;
                        Action::SaveClock
                    }
                },
                _ => Action::None,
            },
            Screen::Alarm(mut ls) => match key {
                Key::Up | Key::Down => {
                    ls.step(if key == Key::Up { -1 } else { 1 }, ALARM_ROWS.len(), ALARM_ROWS.len());
                    self.screen = Screen::Alarm(ls);
                    Action::Redraw
                }
                Key::Left | Key::Cross => {
                    self.screen = Screen::Menu(ListState { sel: 0, top: 0 });
                    Action::Redraw
                }
                Key::Right | Key::Ok | Key::Circle => match ALARM_ROWS[ls.sel] {
                    "Alarm" => {
                        a.on = !a.on;
                        if a.on {
                            a.snooze_until = 0;
                        }
                        self.toast = Some((if a.on { format!("Alarm on {}", a.time_text()) } else { "Alarm off".into() }, 1500));
                        Action::SaveAlarm
                    }
                    "Time" => {
                        self.screen = Screen::AlarmTime(a.hour, a.minute, 0);
                        Action::Redraw
                    }
                    "Repeat" => {
                        a.repeat = a.repeat.next();
                        Action::SaveAlarm
                    }
                    "Snooze" => {
                        let c = alarm::SNOOZE_CHOICES;
                        let i = c.iter().position(|m| *m == a.snooze_min).map(|i| (i + 1) % c.len()).unwrap_or(1);
                        a.snooze_min = c[i];
                        Action::SaveAlarm
                    }
                    _ => {
                        a.snooze_until = now_unix + 5;
                        self.toast = Some(("Rings in 5 s".into(), 1500));
                        Action::SaveAlarm
                    }
                },
                _ => Action::None,
            },
            Screen::AlarmTime(h, m, f) => {
                let back = Screen::Alarm(ListState { sel: 1, top: 0 });
                match key {
                    Key::Up | Key::Down => {
                        let d = if key == Key::Up { 1 } else { -1 };
                        self.screen = if f == 0 {
                            Screen::AlarmTime(wrap(h as i64 + d, 0, 23) as u8, m, f)
                        } else {
                            Screen::AlarmTime(h, wrap(m as i64 + d, 0, 59) as u8, f)
                        };
                        Action::Redraw
                    }
                    Key::Right => {
                        self.screen = Screen::AlarmTime(h, m, 1);
                        Action::Redraw
                    }
                    Key::Left if f > 0 => {
                        self.screen = Screen::AlarmTime(h, m, 0);
                        Action::Redraw
                    }
                    Key::Left | Key::Cross => {
                        self.screen = back;
                        Action::Redraw
                    }
                    Key::Ok | Key::Circle => {
                        a.hour = h;
                        a.minute = m;
                        a.on = true;
                        a.snooze_until = 0;
                        a.last_fired = 0;
                        self.toast = Some((format!("Alarm on {}", a.time_text()), 1500));
                        self.screen = back;
                        Action::SaveAlarm
                    }
                    _ => Action::None,
                }
            }
            Screen::Timer(mm, ss, f) => {
                let back = Screen::Menu(ListState { sel: 1, top: 0 });
                if a.timer_left(now_unix).is_some() {
                    return match key {
                        Key::Ok | Key::Circle => {
                            a.timer_end = 0;
                            self.toast = Some(("Timer stopped".into(), 1500));
                            Action::SaveAlarm
                        }
                        Key::Left | Key::Cross => {
                            self.screen = back;
                            Action::Redraw
                        }
                        _ => Action::None,
                    };
                }
                match key {
                    Key::Up | Key::Down => {
                        let d = if key == Key::Up { 1 } else { -1 };
                        self.screen = if f == 0 {
                            Screen::Timer(wrap(mm as i64 + d, 0, 180) as u32, ss, f)
                        } else {
                            Screen::Timer(mm, wrap(ss as i64 + d * 5, 0, 55) as u32, f)
                        };
                        Action::Redraw
                    }
                    Key::Right => {
                        self.screen = Screen::Timer(mm, ss, 1);
                        Action::Redraw
                    }
                    Key::Left if f > 0 => {
                        self.screen = Screen::Timer(mm, ss, 0);
                        Action::Redraw
                    }
                    Key::Left | Key::Cross => {
                        self.screen = back;
                        Action::Redraw
                    }
                    Key::Ok | Key::Circle => {
                        let secs = mm * 60 + ss;
                        if secs == 0 {
                            self.toast = Some(("Set a time first".into(), 1500));
                            return Action::Redraw;
                        }
                        a.timer_secs = secs;
                        a.timer_end = now_unix + secs as i64;
                        self.toast = Some(("Timer started".into(), 1500));
                        Action::SaveAlarm
                    }
                    _ => Action::None,
                }
            }
        }
    }

    /// Title for the status bar on any screen.
    pub fn title(&self, _s: &Settings, a: &alarm::Settings, now_unix: i64) -> String {
        if let Some(left) = a.timer_left(now_unix) {
            if matches!(self.screen, Screen::Face) {
                return format!("Timer {}", mmss(left));
            }
        }
        match &self.screen {
            Screen::Face if a.on => format!("Alarm {}", a.time_text()),
            Screen::Face => "Clock".into(),
            Screen::Menu(_) => "Clock".into(),
            Screen::Alarm(_) | Screen::AlarmTime(..) => "Alarm".into(),
            Screen::Timer(..) => "Timer".into(),
        }
    }

    /// Draw a non-face screen below the status bar.
    pub fn draw<D: Canvas>(&self, d: &mut D, s: &Settings, a: &alarm::Settings, now: &DateTime, now_unix: i64, volume: Option<u8>) {
        let area = rect(0, STATUS_H, 320, (240 - STATUS_H - HINTS_H) as u32);
        match &self.screen {
            Screen::Face => {}
            Screen::Menu(ls) => {
                let vals: [String; 5] = [
                    if a.on { format!("{}  {}", a.time_text(), a.repeat.label()) } else { "off".into() },
                    a.timer_left(now_unix).map(|l| format!("running {}", mmss(l))).unwrap_or_else(|| mmss(a.timer_secs as i64)),
                    s.face.name().into(),
                    if s.seconds { "on".into() } else { "off".into() },
                    if s.h24 { "24 h".into() } else { "12 h".into() },
                ];
                draw_list(d, area, 30, ls, MENU.len(), |i| Row {
                    title: MENU[i],
                    sub: None,
                    right: Some(vals[i].as_str()),
                    icon: None,
                    mark: if (i == 0 && a.on) || (i == 1 && a.timer_end > 0) { Some(OK) } else { None },
                });
                widgets::hints(d, &[("OK/Rt", "open/change"), ("Lt/X", "back"), ("Home", "top")]);
            }
            Screen::Alarm(ls) => {
                let vals: [String; 5] = [
                    if a.on { "on".into() } else { "off".into() },
                    a.time_text(),
                    a.repeat.label().into(),
                    format!("{} min", a.snooze_min),
                    String::new(),
                ];
                let list = rect(0, STATUS_H, 320, 150);
                draw_list(d, list, 30, ls, ALARM_ROWS.len(), |i| Row {
                    title: ALARM_ROWS[i],
                    sub: None,
                    right: Some(vals[i].as_str()).filter(|v| !v.is_empty()),
                    icon: None,
                    mark: if i == 0 && a.on { Some(OK) } else { None },
                });
                let _ = d.fill_solid(&rect(0, STATUS_H + 150, 320, (240 - STATUS_H - HINTS_H - 150) as u32), BG);
                let next = format!("Next: {}", a.next_text(now));
                text::draw_aligned(d, Font::BodyBold, &next, 160, STATUS_H + 156, Align::Center, if a.on { ACCENT } else { DIM });
                let vol = match volume {
                    Some(v) if v < 60 => (format!("Rings at 60% (now {v}%), only while on"), FAINT),
                    Some(v) => (format!("Rings at {v}%, only while the ZPOD is on"), FAINT),
                    None => ("Rings only while the ZPOD is on".into(), FAINT),
                };
                text::draw_aligned(d, Font::Small, &vol.0, 160, STATUS_H + 176, Align::Center, vol.1);
                widgets::hints(d, &[("OK", "change"), ("Lt/X", "back"), ("Home", "top")]);
            }
            Screen::AlarmTime(h, m, f) => {
                let _ = d.fill_solid(&area, BG);
                editor(d, &[format!("{h:02}"), format!("{m:02}")], &["Hour", "Minute"], *f, ":");
                text::draw_aligned(d, Font::Body, "OK = set alarm (switches it on)", 160, STATUS_H + 140, Align::Center, DIM);
                widgets::hints(d, &[("Up/Dn", "change"), ("Lt/Rt", "field"), ("OK", "save"), ("X", "cancel")]);
            }
            Screen::Timer(mm, ss, f) => {
                let _ = d.fill_solid(&area, BG);
                match a.timer_left(now_unix) {
                    Some(left) => {
                        let t = mmss(left);
                        text::draw_aligned(d, Font::Digits58, &t, 160, STATUS_H + 40, Align::Center, if left <= 10 { WARN } else { WHITE });
                        let total = a.timer_secs.max(1) as f32;
                        widgets::progress_bar(d, 30, STATUS_H + 120, 260, 8, 1.0 - left as f32 / total, OK, PANEL);
                        text::draw_aligned(d, Font::Body, &format!("of {}  \u{b7}  rings outside Clock too", mmss(a.timer_secs as i64)), 160, STATUS_H + 140, Align::Center, DIM);
                        widgets::hints(d, &[("OK", "stop"), ("Lt/X", "back (keeps running)"), ("Home", "top")]);
                    }
                    None => {
                        editor(d, &[format!("{mm:02}"), format!("{ss:02}")], &["Minutes", "Seconds"], *f, ":");
                        text::draw_aligned(d, Font::Body, "OK = start the countdown", 160, STATUS_H + 140, Align::Center, DIM);
                        widgets::hints(d, &[("Up/Dn", "change"), ("Lt/Rt", "field"), ("OK", "start"), ("X", "back")]);
                    }
                }
            }
        }
        if let Some((msg, _)) = &self.toast {
            widgets::toast(d, msg);
        }
    }
}

pub fn mmss(secs: i64) -> String {
    let s = secs.max(0);
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{:02}:{:02}", s / 60, s % 60)
    }
}

/// Two big fields with a separator, the selected one highlighted.
fn editor<D: Canvas>(d: &mut D, vals: &[String; 2], names: &[&str; 2], field: usize, sep: &str) {
    let gap = text::width(Font::Digits58, sep) + 16;
    let w0 = text::width(Font::Digits58, &vals[0]);
    let w1 = text::width(Font::Digits58, &vals[1]);
    let mut x = (320 - (w0 + w1 + gap)) / 2;
    let y = STATUS_H + 36;
    for i in 0..2 {
        let w = if i == 0 { w0 } else { w1 };
        if i == field {
            zpod_ui::fill(d, x - 6, y - 6, (w + 12) as u32, 72, SEL);
        }
        text::draw(d, Font::Digits58, &vals[i], x, y, if i == field { WHITE } else { FG });
        text::draw_aligned(d, Font::Tiny, names[i], x + w / 2, y + 70, Align::Center, DIM);
        x += w;
        if i == 0 {
            text::draw(d, Font::Digits58, sep, x + 8, y, DIM);
            x += gap;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clock_core::Face;
    use zpod_hal::alarm::Repeat;

    fn k(ui: &mut Ui, key: Key, s: &mut Settings, a: &mut alarm::Settings) -> Action {
        ui.key(key, KeyState::Press, s, a, 1_000_000)
    }

    #[test]
    fn face_keys_and_menu() {
        let (mut ui, mut s, mut a) = (Ui::default(), Settings::default(), alarm::Settings::default());
        assert_eq!(k(&mut ui, Key::Down, &mut s, &mut a), Action::SaveClock);
        assert_eq!(s.face, Face::Analog);
        assert_eq!(k(&mut ui, Key::Square, &mut s, &mut a), Action::SaveClock);
        assert!(!s.seconds);
        assert_eq!(k(&mut ui, Key::Ok, &mut s, &mut a), Action::Redraw);
        assert!(matches!(ui.screen, Screen::Menu(_)));
        assert_eq!(k(&mut ui, Key::Left, &mut s, &mut a), Action::ToFace);
        assert_eq!(k(&mut ui, Key::Cross, &mut s, &mut a), Action::Exit);
        assert_eq!(k(&mut ui, Key::Home, &mut s, &mut a), Action::Exit);
        assert_eq!(k(&mut ui, Key::PlayPause, &mut s, &mut a), Action::PlayPause);
    }

    #[test]
    fn set_alarm_time_and_options() {
        let (mut ui, mut s, mut a) = (Ui::default(), Settings::default(), alarm::Settings::default());
        k(&mut ui, Key::Right, &mut s, &mut a); // menu
        k(&mut ui, Key::Ok, &mut s, &mut a); // Alarm
        assert!(matches!(ui.screen, Screen::Alarm(_)));
        k(&mut ui, Key::Down, &mut s, &mut a); // Time
        k(&mut ui, Key::Ok, &mut s, &mut a);
        assert_eq!(ui.screen, Screen::AlarmTime(7, 0, 0));
        k(&mut ui, Key::Down, &mut s, &mut a); // 06
        k(&mut ui, Key::Right, &mut s, &mut a);
        for _ in 0..3 {
            k(&mut ui, Key::Down, &mut s, &mut a); // 57
        }
        assert_eq!(k(&mut ui, Key::Ok, &mut s, &mut a), Action::SaveAlarm);
        assert_eq!((a.hour, a.minute, a.on), (6, 57, true));
        assert!(matches!(ui.screen, Screen::Alarm(ListState { sel: 1, .. })));
        k(&mut ui, Key::Down, &mut s, &mut a); // Repeat
        k(&mut ui, Key::Ok, &mut s, &mut a);
        assert_eq!(a.repeat, Repeat::Weekdays);
        k(&mut ui, Key::Down, &mut s, &mut a); // Snooze 9 -> 10
        k(&mut ui, Key::Ok, &mut s, &mut a);
        assert_eq!(a.snooze_min, 10);
        k(&mut ui, Key::Down, &mut s, &mut a); // test ring
        k(&mut ui, Key::Ok, &mut s, &mut a);
        assert_eq!(a.snooze_until, 1_000_005);
        // X in the time editor cancels
        ui.screen = Screen::AlarmTime(1, 2, 1);
        k(&mut ui, Key::Cross, &mut s, &mut a);
        assert_eq!((a.hour, a.minute), (6, 57));
        // Alarm row toggles off
        ui.screen = Screen::Alarm(ListState::default());
        k(&mut ui, Key::Ok, &mut s, &mut a);
        assert!(!a.on);
    }

    #[test]
    fn timer_start_and_stop() {
        let (mut ui, mut s, mut a) = (Ui::default(), Settings::default(), alarm::Settings::default());
        k(&mut ui, Key::Ok, &mut s, &mut a);
        k(&mut ui, Key::Down, &mut s, &mut a);
        k(&mut ui, Key::Ok, &mut s, &mut a); // Timer, 05:00
        assert_eq!(ui.screen, Screen::Timer(5, 0, 0));
        k(&mut ui, Key::Up, &mut s, &mut a); // 06:00
        k(&mut ui, Key::Right, &mut s, &mut a);
        k(&mut ui, Key::Up, &mut s, &mut a); // 06:05
        assert_eq!(k(&mut ui, Key::Ok, &mut s, &mut a), Action::SaveAlarm);
        assert_eq!(a.timer_end, 1_000_000 + 365);
        assert_eq!(a.timer_secs, 365);
        assert!(ui.title(&s, &a, 1_000_000).starts_with("Timer"));
        k(&mut ui, Key::Ok, &mut s, &mut a); // running: OK stops
        assert_eq!(a.timer_end, 0);
        ui.screen = Screen::Timer(0, 0, 0);
        k(&mut ui, Key::Ok, &mut s, &mut a);
        assert_eq!(a.timer_end, 0); // zero length refused
        assert_eq!(mmss(3725), "1:02:05");
    }
}
