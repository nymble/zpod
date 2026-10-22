//! Clock faces (`no_std` + `alloc`), drawn on any Rgb565 `DrawTarget`.
//!
//! Each face draws only what changed since the previous call (it remembers
//! the last minute/second it drew), so a seconds-off clock touches the
//! panel once a minute.
#![no_std]
extern crate alloc;

use alloc::format;
use embedded_graphics::{
    pixelcolor::Rgb565,
    prelude::*,
    primitives::{Circle, Line, PrimitiveStyle, PrimitiveStyleBuilder},
};
use zpod_core::DateTime;
use zpod_ui::{fill, text, text::Align, theme, Canvas, Font};

pub const TOP: i32 = theme::STATUS_H;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face {
    Digital,
    Analog,
    Words,
}

pub const FACES: [Face; 3] = [Face::Digital, Face::Analog, Face::Words];

impl Face {
    pub fn name(self) -> &'static str {
        match self {
            Face::Digital => "Digital",
            Face::Analog => "Analog",
            Face::Words => "Words",
        }
    }
    pub fn index(self) -> usize {
        FACES.iter().position(|f| *f == self).unwrap_or(0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub face: Face,
    pub seconds: bool,
    pub h24: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { face: Face::Digital, seconds: true, h24: true }
    }
}

impl Settings {
    pub fn parse(s: &str) -> Self {
        let mut st = Settings::default();
        for line in s.lines() {
            match line.split_once('=').map(|(a, b)| (a.trim(), b.trim())) {
                Some(("face", v)) => st.face = FACES.iter().copied().find(|f| f.name().eq_ignore_ascii_case(v)).unwrap_or(st.face),
                Some(("seconds", v)) => st.seconds = v == "1" || v == "true",
                Some(("h24", v)) => st.h24 = v == "1" || v == "true",
                _ => {}
            }
        }
        st
    }
    pub fn to_text(&self) -> alloc::string::String {
        format!("face={}\nseconds={}\nh24={}\n", self.face.name(), self.seconds as u8, self.h24 as u8)
    }
}

/// sin/cos without libm (range-reduced Taylor series, |err| < 1e-4).
pub fn sin_cos(deg: f32) -> (f32, f32) {
    fn sin_r(x: f32) -> f32 {
        // x in [-pi, pi] -> fold into [-pi/2, pi/2]
        let h = core::f32::consts::FRAC_PI_2;
        let x = if x > h { core::f32::consts::PI - x } else if x < -h { -core::f32::consts::PI - x } else { x };
        let x2 = x * x;
        x * (1.0 - x2 / 6.0 * (1.0 - x2 / 20.0 * (1.0 - x2 / 42.0 * (1.0 - x2 / 72.0))))
    }
    let mut d = deg % 360.0;
    if d < 0.0 {
        d += 360.0;
    }
    let to_r = |d: f32| {
        let d = if d > 180.0 { d - 360.0 } else { d };
        d * core::f32::consts::PI / 180.0
    };
    (sin_r(to_r(d)), sin_r(to_r((d + 90.0) % 360.0)))
}

/// What a face last drew (to redraw only changes).
#[derive(Default)]
pub struct Painter {
    last_min: Option<(i32, u8, u8, u8, u8)>,
    last_sec: Option<u8>,
    last_face: Option<Face>,
    last_seconds: Option<bool>,
}

impl Painter {
    pub fn invalidate(&mut self) {
        *self = Painter::default();
    }

    /// Draw `t` with `s`. Returns true if anything changed.
    pub fn draw<D: Canvas>(&mut self, d: &mut D, t: &DateTime, s: &Settings) -> bool {
        let full = self.last_face != Some(s.face) || self.last_seconds != Some(s.seconds);
        if full {
            fill(d, 0, TOP, 320, (240 - TOP) as u32, theme::BLACK);
            self.last_min = None;
            self.last_sec = None;
            self.last_face = Some(s.face);
            self.last_seconds = Some(s.seconds);
        }
        let key = (t.year, t.month, t.day, t.hour, t.minute);
        let min_changed = self.last_min != Some(key);
        let sec_changed = s.seconds && self.last_sec != Some(t.second);
        if !min_changed && !sec_changed {
            return false;
        }
        match s.face {
            Face::Digital => digital(d, t, s, min_changed),
            Face::Analog => analog(d, t, s),
            Face::Words => words(d, t, s, min_changed),
        }
        self.last_min = Some(key);
        self.last_sec = Some(t.second);
        true
    }
}

fn hour_text(t: &DateTime, h24: bool) -> alloc::string::String {
    if h24 {
        format!("{:02}:{:02}", t.hour, t.minute)
    } else {
        let h = if t.hour % 12 == 0 { 12 } else { t.hour % 12 };
        format!("{}:{:02}", h, t.minute)
    }
}

// ---- 1. Large digital --------------------------------------------------
fn digital<D: Canvas>(d: &mut D, t: &DateTime, s: &Settings, min_changed: bool) {
    if min_changed {
        fill(d, 0, TOP, 320, 200, theme::BLACK);
        let hm = hour_text(t, s.h24);
        let w = text::width(Font::Digits92, &hm);
        let x = if s.seconds { (276 - w) / 2 } else { (320 - w) / 2 };
        text::draw(d, Font::Digits92, &hm, x.max(2), TOP + 34, theme::WHITE);
        if !s.h24 {
            text::draw(d, Font::BodyBold, if t.hour < 12 { "AM" } else { "PM" }, 280, TOP + 40, theme::DIM);
        }
        let date = format!("{}, {} {}", t.weekday_name(), t.month_name(), t.day);
        text::draw_aligned(d, Font::Large, &date, 160, TOP + 150, Align::Center, theme::ACCENT);
    }
    if s.seconds {
        // seconds digits + a 60-step bar
        fill(d, 270, TOP + 96, 50, 34, theme::BLACK);
        let ss = format!("{:02}", t.second);
        text::draw(d, Font::Digits32, &ss, 272, TOP + 96, theme::CYAN);
        let y = 240 - 12;
        fill(d, 20, y, 280, 4, theme::rgb(24, 28, 44));
        fill(d, 20, y, (280 * (t.second as u32 + 1) / 60).max(1), 4, theme::CYAN);
    }
}

// ---- 2. Analog -----------------------------------------------------------
const CX: i32 = 160;
const CY: i32 = TOP + 110;
const R: i32 = 104;

fn hand<D: Canvas>(d: &mut D, deg: f32, len: i32, tail: i32, w: u32, c: Rgb565) {
    let (s, co) = sin_cos(deg);
    let p = |l: i32| Point::new(CX + (s * l as f32) as i32, CY - (co * l as f32) as i32);
    let _ = Line::new(p(-tail), p(len)).into_styled(PrimitiveStyle::with_stroke(c, w)).draw(d);
}

fn analog<D: Canvas>(d: &mut D, t: &DateTime, s: &Settings) {
    fill(d, CX - R - 2, CY - R - 2, (2 * R + 5) as u32, (2 * R + 5) as u32, theme::BLACK);
    let face = PrimitiveStyleBuilder::new().fill_color(theme::rgb(14, 16, 30)).stroke_color(theme::FAINT).stroke_width(2).build();
    let _ = Circle::with_center(Point::new(CX, CY), (2 * R) as u32).into_styled(face).draw(d);
    for i in 0..60 {
        let (sn, cs) = sin_cos(i as f32 * 6.0);
        let (r0, w, c) = if i % 5 == 0 { (R - 14, 3, theme::FG) } else { (R - 6, 1, theme::DIM) };
        let a = Point::new(CX + (sn * r0 as f32) as i32, CY - (cs * r0 as f32) as i32);
        let b = Point::new(CX + (sn * (R - 3) as f32) as i32, CY - (cs * (R - 3) as f32) as i32);
        let _ = Line::new(a, b).into_styled(PrimitiveStyle::with_stroke(c, w)).draw(d);
    }
    for (n, label) in [(0, "12"), (3, "3"), (6, "6"), (9, "9")] {
        let (sn, cs) = sin_cos(n as f32 * 30.0);
        let r = (R - 30) as f32;
        let x = CX + (sn * r) as i32;
        let y = CY - (cs * r) as i32 - 8;
        text::draw_aligned(d, Font::Large, label, x, y, Align::Center, theme::DIM);
    }
    let date = format!("{} {}", &t.weekday_name()[..3], t.day);
    fill(d, CX - 28, CY + 30, 56, 18, theme::rgb(30, 34, 60));
    text::draw_aligned(d, Font::BodyBold, &date, CX, CY + 33, Align::Center, theme::ACCENT);
    let m = t.minute as f32 + if s.seconds { t.second as f32 / 60.0 } else { 0.0 };
    let h = (t.hour % 12) as f32 + m / 60.0;
    hand(d, h * 30.0, 56, 10, 7, theme::FG);
    hand(d, m * 6.0, 86, 12, 4, theme::FG);
    if s.seconds {
        hand(d, t.second as f32 * 6.0, 94, 18, 1, theme::WARN);
    }
    let _ = Circle::with_center(Point::new(CX, CY), 9).into_styled(PrimitiveStyle::with_fill(if s.seconds { theme::WARN } else { theme::FG })).draw(d);
}

// ---- 3. Word clock --------------------------------------------------------
pub const GRID: [&str; 10] = [
    "ITLISASTIME",
    "ACQUARTERDC",
    "TWENTYFIVEX",
    "HALFSTENFTO",
    "PASTERUNINE",
    "ONESIXTHREE",
    "FOURFIVETWO",
    "EIGHTELEVEN",
    "SEVENTWELVE",
    "TENSEOCLOCK",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Word(pub u8, pub u8, pub u8); // row, col, len

const IT: Word = Word(0, 0, 2);
const IS: Word = Word(0, 3, 2);
const A: Word = Word(1, 0, 1);
const QUARTER: Word = Word(1, 2, 7);
const TWENTY: Word = Word(2, 0, 6);
const FIVE_M: Word = Word(2, 6, 4);
const HALF: Word = Word(3, 0, 4);
const TEN_M: Word = Word(3, 5, 3);
const TO: Word = Word(3, 9, 2);
const PAST: Word = Word(4, 0, 4);
const OCLOCK: Word = Word(9, 5, 6);
const HOURS: [Word; 12] = [
    Word(8, 5, 6), // twelve
    Word(5, 0, 3), // one
    Word(6, 8, 3), // two
    Word(5, 6, 5), // three
    Word(6, 0, 4), // four
    Word(6, 4, 4), // five
    Word(5, 3, 3), // six
    Word(8, 0, 5), // seven
    Word(7, 0, 5), // eight
    Word(4, 7, 4), // nine
    Word(9, 0, 3), // ten
    Word(7, 5, 6), // eleven
];

/// Words lit for a time (5-minute resolution) + extra minute dots (0..4).
pub fn words_for(hour: u8, minute: u8) -> (alloc::vec::Vec<Word>, u8) {
    let m5 = minute / 5;
    let mut v = alloc::vec![IT, IS];
    match m5 {
        0 => {}
        1 | 11 => v.push(FIVE_M),
        2 | 10 => v.push(TEN_M),
        3 | 9 => v.extend([A, QUARTER]),
        4 | 8 => v.push(TWENTY),
        5 | 7 => v.extend([TWENTY, FIVE_M]),
        _ => v.push(HALF),
    }
    match m5 {
        0 => {}
        1..=6 => v.push(PAST),
        _ => v.push(TO),
    }
    let h = if m5 >= 7 { hour + 1 } else { hour };
    v.push(HOURS[(h % 12) as usize]);
    if m5 == 0 {
        v.push(OCLOCK);
    }
    (v, minute % 5)
}

pub fn spell(ws: &[Word]) -> alloc::string::String {
    let mut s = alloc::string::String::new();
    for w in ws {
        if !s.is_empty() {
            s.push(' ');
        }
        s.push_str(&GRID[w.0 as usize][w.1 as usize..(w.1 + w.2) as usize]);
    }
    s
}

fn words<D: Canvas>(d: &mut D, t: &DateTime, s: &Settings, min_changed: bool) {
    let (cw, ch) = (26, 20);
    let x0 = (320 - 11 * cw) / 2;
    let y0 = TOP + 10;
    if min_changed {
        fill(d, 0, TOP, 320, 206, theme::BLACK);
        let (lit, dots) = words_for(t.hour, t.minute);
        for (r, row) in GRID.iter().enumerate() {
            for (c, chr) in row.chars().enumerate() {
                let on = lit.iter().any(|w| w.0 as usize == r && (w.1 as usize..(w.1 + w.2) as usize).contains(&c));
                let mut b = [0u8; 4];
                let col = if on { theme::ACCENT } else { theme::rgb(40, 42, 60) };
                text::draw_aligned(d, Font::Title, chr.encode_utf8(&mut b), x0 + c as i32 * cw + cw / 2, y0 + r as i32 * ch, Align::Center, col);
            }
        }
        // minute dots in the four corners
        let corners = [(4, TOP + 4), (312, TOP + 4), (312, 236 - 16), (4, 236 - 16)];
        for (i, (x, y)) in corners.iter().enumerate() {
            let c = if (i as u8) < dots { theme::ACCENT } else { theme::rgb(30, 32, 46) };
            fill(d, *x, *y, 4, 4, c);
        }
    }
    if s.seconds {
        let y = 240 - 8;
        fill(d, 20, y, 280, 3, theme::rgb(24, 28, 44));
        fill(d, 20, y, (280 * (t.second as u32 + 1) / 60).max(1), 3, theme::ACCENT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_clock_phrases() {
        let f = |h, m| {
            let (w, d) = words_for(h, m);
            (spell(&w), d)
        };
        assert_eq!(f(10, 8), ("IT IS FIVE PAST TEN".into(), 3));
        assert_eq!(f(0, 0), ("IT IS TWELVE OCLOCK".into(), 0));
        assert_eq!(f(15, 45), ("IT IS A QUARTER TO FOUR".into(), 0));
        assert_eq!(f(23, 37), ("IT IS TWENTY FIVE TO TWELVE".into(), 2));
        assert_eq!(f(9, 30), ("IT IS HALF PAST NINE".into(), 0));
        assert_eq!(f(11, 55), ("IT IS FIVE TO TWELVE".into(), 0));
        assert_eq!(f(7, 20), ("IT IS TWENTY PAST SEVEN".into(), 0));
        assert_eq!(f(6, 50), ("IT IS TEN TO SEVEN".into(), 0));
        assert_eq!(f(13, 25), ("IT IS TWENTY FIVE PAST ONE".into(), 0));
    }

    #[test]
    fn trig() {
        for deg in [0.0f32, 30.0, 90.0, 135.0, 180.0, 270.0, 359.0, -90.0, 725.0] {
            let (s, c) = sin_cos(deg);
            let r = deg.to_radians();
            assert!((s - r.sin()).abs() < 2e-3 && (c - r.cos()).abs() < 2e-3, "{deg}: {s} {c}");
        }
    }

    #[test]
    fn settings_roundtrip() {
        let s = Settings { face: Face::Words, seconds: false, h24: false };
        assert_eq!(Settings::parse(&s.to_text()), s);
        assert_eq!(Settings::parse("garbage"), Settings::default());
    }

    #[test]
    fn redraws_only_on_change() {
        let mut fb = zpod_ui::Framebuffer::default();
        let mut p = Painter::default();
        let s = Settings { face: Face::Digital, seconds: false, h24: true };
        let t = DateTime::parse("2026-10-17 10:08:05").unwrap();
        assert!(p.draw(&mut fb, &t, &s));
        fb.take_dirty();
        let t2 = DateTime { second: 30, ..t };
        assert!(!p.draw(&mut fb, &t2, &s)); // seconds off: nothing to do
        assert!(!fb.is_dirty());
        let s2 = Settings { seconds: true, ..s.clone() };
        assert!(p.draw(&mut fb, &t2, &s2));
        fb.take_dirty();
        let t3 = DateTime { second: 31, ..t };
        assert!(p.draw(&mut fb, &t3, &s2));
        // only the seconds area + bar changed, not the whole screen
        let area: u32 = fb.dirty_rects().iter().map(|r| r.area()).sum();
        assert!(area < 320 * 60, "dirty area {area}");
    }
}
