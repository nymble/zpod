//! Text entry for any app: a [`TextField`] typed on an optional USB /
//! Bluetooth keyboard, or picked character by character with the buttons
//! (arrows + OK/Circle) from an on-screen grid. Reusable: Wi-Fi passwords
//! first, but nothing here is Wi-Fi specific.
//!
//! * Character sets (tabs): `abc`, `ABC`, `123` (digits + common symbols),
//!   `#+=` (the other symbols), plus a Space cell: every printable ASCII
//!   character, 0x20..=0x7E, is reachable with the buttons.
//! * The keyboard is optional and hot-pluggable: the app reports whether one
//!   is present ([`TextEntry::set_keyboard`]); without one the grid is
//!   always shown, with one the keyboard view is shown until an arrow is
//!   pressed. Typing and picking edit the same field, so unplugging the
//!   keyboard in the middle keeps what was typed.
//! * Buttons on the grid: arrows move (wrapping), OK/Circle presses the
//!   cell, X deletes the last character (X on an empty field cancels),
//!   Square switches to the next character set, Triangle shows / hides.
//! * Secrets: the field is masked by default; neither this type's `Debug`
//!   nor any [`Outcome`] carries the content; the app takes it once with
//!   `field.take()` (which wipes the buffer).

use alloc::vec::Vec;
use embedded_graphics::{
    prelude::*,
    primitives::{PrimitiveStyle, RoundedRectangle},
};

use crate::textfield::{draw_field, draw_text_entry, Edit, EntryNote, TextField};
use crate::{fill, rect, text, text::Align, theme::*, Canvas, Font};

pub const LOWER: &str = "abcdefghijklmnopqrstuvwxyz";
pub const UPPER: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
/// Digits and the symbols passwords use most.
pub const DIGITS_SYM: &str = "1234567890!@#$%^&*()-_=+.,:;?/";
/// The remaining printable ASCII symbols.
pub const MORE_SYM: &str = "'\"`~\\|<>[]{}";

/// (tab label, characters)
pub const SETS: [(&str, &str); 4] = [("abc", LOWER), ("ABC", UPPER), ("123", DIGITS_SYM), ("#+=", MORE_SYM)];
pub const COLS: usize = 10;

/// Buttons, already decoded by the app (any repeat policy).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pad {
    Up,
    Down,
    Left,
    Right,
    /// OK or Circle
    Select,
    /// X
    Back,
    /// Square
    NextSet,
    /// Triangle
    Reveal,
}

/// Keyboard keys, already decoded by the app (US layout text).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyIn {
    Char(char),
    Enter,
    Esc,
    Backspace,
    /// Ctrl+Backspace
    ClearAll,
    /// Tab: show / hide
    Tab,
}

/// What the app should do after an input. Never carries the text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing changed.
    None,
    /// Redraw (moved, typed, deleted, toggled).
    Redraw,
    /// Use `field` (validate, then `field.take()`).
    Submit,
    /// Leave; the app drops the entry (wipes the field).
    Cancel,
    /// The "USB keyboard" cell: the app asks to switch the USB port to host
    /// mode through its own confirmation screen.
    WantKeyboard,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cell {
    Char(char),
    Tab(usize),
    Space,
    Delete,
    Keyboard,
    Done,
}

/// Selection in the on-screen grid (no text).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Picker {
    pub set: usize,
    pub row: usize,
    pub col: usize,
    /// Show the "USB keyboard" cell (no keyboard and the port is not a host).
    pub offer_keyboard: bool,
    /// x the user is moving up/down along (so a round trip lands where it began)
    want_x: Option<i32>,
}

/// Geometry: character rows, then the action row.
pub const GRID_X: i32 = 10;
pub const CELL_PITCH: i32 = 30;
pub const ROW_PITCH: i32 = 28;
pub const CELL_H: u32 = 25;
pub const GRID_Y: i32 = STATUS_H + 70;
/// The tallest set (30 characters = 3 rows of 10).
pub const MAX_CHAR_ROWS: usize = 3;

impl Picker {
    fn chars(&self) -> &'static str {
        SETS[self.set].1
    }

    /// Character rows of the current set.
    pub fn char_rows(&self) -> usize {
        self.chars().len().div_ceil(COLS)
    }

    pub fn rows(&self) -> usize {
        self.char_rows() + 1
    }

    pub fn action_cells(&self) -> Vec<(Cell, u32)> {
        let mut v: Vec<(Cell, u32)> = (0..SETS.len()).map(|i| (Cell::Tab(i), 32)).collect();
        v.push((Cell::Space, 52));
        v.push((Cell::Delete, 34));
        if self.offer_keyboard {
            v.push((Cell::Keyboard, 34));
        }
        v.push((Cell::Done, 46));
        v
    }

    pub fn row_len(&self, r: usize) -> usize {
        let cr = self.char_rows();
        if r < cr {
            (self.chars().len() - r * COLS).min(COLS)
        } else {
            self.action_cells().len()
        }
    }

    pub fn cell(&self, r: usize, c: usize) -> Option<Cell> {
        if r < self.char_rows() {
            self.chars().chars().nth(r * COLS + c).map(Cell::Char)
        } else {
            self.action_cells().get(c).map(|x| x.0)
        }
    }

    pub fn selected(&self) -> Option<Cell> {
        self.cell(self.row, self.col)
    }

    /// (x, y, w, h) of a cell on the 320x240 screen.
    pub fn cell_rect(&self, r: usize, c: usize) -> (i32, i32, u32, u32) {
        // the action row stays at the bottom whatever the set's height
        let y = if r < self.char_rows() { GRID_Y + r as i32 * ROW_PITCH } else { GRID_Y + MAX_CHAR_ROWS as i32 * ROW_PITCH + 2 };
        if r < self.char_rows() {
            (GRID_X + c as i32 * CELL_PITCH, y, (CELL_PITCH - 2) as u32, CELL_H)
        } else {
            let cells = self.action_cells();
            let total: i32 = cells.iter().map(|(_, w)| *w as i32 + 2).sum::<i32>() - 2;
            let mut x = (320 - total) / 2;
            for (i, (_, w)) in cells.iter().enumerate() {
                if i == c {
                    return (x, y, *w, CELL_H);
                }
                x += *w as i32 + 2;
            }
            (x, y, 0, CELL_H)
        }
    }

    fn centre_x(&self, r: usize, c: usize) -> i32 {
        let (x, _, w, _) = self.cell_rect(r, c);
        x + w as i32 / 2
    }

    /// Column in row `r` nearest to x (keeps the column when moving up/down).
    fn nearest(&self, r: usize, x: i32) -> usize {
        (0..self.row_len(r)).min_by_key(|&c| (self.centre_x(r, c) - x).abs()).unwrap_or(0)
    }

    fn clamp(&mut self) {
        if self.row >= self.rows() {
            self.row = self.rows() - 1;
        }
        let n = self.row_len(self.row);
        if self.col >= n {
            self.col = n.saturating_sub(1);
        }
    }

    pub fn set_offer_keyboard(&mut self, on: bool) {
        if self.offer_keyboard != on {
            let on_actions = self.row == self.char_rows();
            let sel = self.selected();
            self.offer_keyboard = on;
            if on_actions {
                // keep the same action selected
                let cells = self.action_cells();
                self.col = cells.iter().position(|(c, _)| Some(*c) == sel).unwrap_or(0);
            }
            self.clamp();
        }
    }

    /// Switch to set `i`, staying on the action row if there.
    pub fn select_set(&mut self, i: usize) {
        let on_actions = self.row == self.char_rows();
        let x = self.centre_x(self.row, self.col);
        self.set = i % SETS.len();
        if on_actions {
            self.row = self.char_rows();
            self.col = self.action_cells().iter().position(|(c, _)| *c == Cell::Tab(self.set)).unwrap_or(0);
        } else {
            // stay on a character row (the new set may have fewer)
            self.row = self.row.min(self.char_rows() - 1);
            self.col = self.nearest(self.row, x);
        }
    }

    /// Move; returns true if the selection changed.
    pub fn step(&mut self, p: Pad) -> bool {
        let before = (self.row, self.col);
        let rows = self.rows();
        let x = self.want_x.unwrap_or_else(|| self.centre_x(self.row, self.col));
        match p {
            Pad::Left => {
                self.want_x = None;
                let n = self.row_len(self.row);
                self.col = (self.col + n - 1) % n;
            }
            Pad::Right => {
                self.want_x = None;
                let n = self.row_len(self.row);
                self.col = (self.col + 1) % n;
            }
            Pad::Up => {
                self.want_x = Some(x);
                self.row = (self.row + rows - 1) % rows;
                self.col = self.nearest(self.row, x);
            }
            Pad::Down => {
                self.want_x = Some(x);
                self.row = (self.row + 1) % rows;
                self.col = self.nearest(self.row, x);
            }
            _ => {}
        }
        before != (self.row, self.col)
    }

    /// Shortest button sequence (arrows and Square) that selects `target`
    /// from here; the caller then presses Select. For demos, scripted
    /// screenshots and tests.
    pub fn presses_to(&self, target: Cell) -> Option<Vec<Pad>> {
        use alloc::collections::{BTreeMap, VecDeque};
        let key = |p: &Picker| (p.set, p.row, p.col, p.want_x);
        let mut seen: BTreeMap<(usize, usize, usize, Option<i32>), ()> = BTreeMap::new();
        let mut q: VecDeque<(Picker, Vec<Pad>)> = VecDeque::new();
        seen.insert(key(self), ());
        q.push_back((*self, Vec::new()));
        while let Some((p, path)) = q.pop_front() {
            if p.selected() == Some(target) {
                return Some(path);
            }
            if path.len() > 40 {
                continue;
            }
            for m in [Pad::Right, Pad::Left, Pad::Down, Pad::Up, Pad::NextSet] {
                let mut n = p;
                if m == Pad::NextSet {
                    n.select_set(n.set + 1);
                } else {
                    n.step(m);
                }
                if seen.insert(key(&n), ()).is_none() {
                    let mut np = path.clone();
                    np.push(m);
                    q.push_back((n, np));
                }
            }
        }
        None
    }

    /// Where `ch` is: (set, row, col).
    pub fn find(ch: char) -> Option<(usize, usize, usize)> {
        SETS.iter().enumerate().find_map(|(s, (_, cs))| cs.chars().position(|c| c == ch).map(|i| (s, i / COLS, i % COLS)))
    }
}

/// A text field with keyboard and on-screen entry.
pub struct TextEntry {
    pub field: TextField,
    pub pick: Picker,
    keyboard: bool,
    /// The user pressed an arrow while a keyboard was present.
    picker_on: bool,
}

impl core::fmt::Debug for TextEntry {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TextEntry").field("field", &self.field).field("keyboard", &self.keyboard).field("picker", &self.showing_picker()).finish()
    }
}

impl TextEntry {
    pub fn new(field: TextField) -> Self {
        TextEntry { field, pick: Picker::default(), keyboard: false, picker_on: false }
    }

    /// Masked Wi-Fi passphrase (printable ASCII, max 63).
    pub fn wifi_password() -> Self {
        TextEntry::new(TextField::wifi_password())
    }

    pub fn keyboard(&self) -> bool {
        self.keyboard
    }

    /// The grid is on screen (no keyboard, or the user chose the buttons).
    pub fn showing_picker(&self) -> bool {
        !self.keyboard || self.picker_on
    }

    /// Report whether a keyboard is connected now (call every loop; hotplug).
    /// Returns true when the screen should be redrawn.
    pub fn set_keyboard(&mut self, present: bool) -> bool {
        if present == self.keyboard {
            return false;
        }
        self.keyboard = present;
        // plugged in: keyboard view; unplugged: the grid (always shown then)
        self.picker_on = false;
        true
    }

    fn edit(&mut self, e: Edit) -> Outcome {
        if self.field.apply(e) {
            Outcome::Redraw
        } else {
            Outcome::None
        }
    }

    /// A button.
    pub fn pad(&mut self, p: Pad) -> Outcome {
        if !self.showing_picker() {
            // keyboard view: OK joins, X cancels, Triangle shows; an arrow or
            // Square brings up the grid
            return match p {
                Pad::Select => Outcome::Submit,
                Pad::Back => Outcome::Cancel,
                Pad::Reveal => self.edit(Edit::ToggleReveal),
                _ => {
                    self.picker_on = true;
                    Outcome::Redraw
                }
            };
        }
        match p {
            Pad::Up | Pad::Down | Pad::Left | Pad::Right => {
                if self.pick.step(p) {
                    Outcome::Redraw
                } else {
                    Outcome::None
                }
            }
            Pad::NextSet => {
                self.pick.select_set(self.pick.set + 1);
                Outcome::Redraw
            }
            Pad::Reveal => self.edit(Edit::ToggleReveal),
            Pad::Back => {
                if self.field.is_empty() {
                    Outcome::Cancel
                } else {
                    self.edit(Edit::Backspace)
                }
            }
            Pad::Select => match self.pick.selected() {
                Some(Cell::Char(c)) => self.edit(Edit::Insert(c)),
                Some(Cell::Space) => self.edit(Edit::Insert(' ')),
                Some(Cell::Delete) => self.edit(Edit::Backspace),
                Some(Cell::Tab(i)) => {
                    self.pick.select_set(i);
                    Outcome::Redraw
                }
                Some(Cell::Keyboard) => Outcome::WantKeyboard,
                Some(Cell::Done) => Outcome::Submit,
                None => Outcome::None,
            },
        }
    }

    /// A keyboard key (typing works in both views).
    pub fn key(&mut self, k: KeyIn) -> Outcome {
        let was_picker = self.showing_picker();
        if self.keyboard {
            self.picker_on = false; // typing: back to the keyboard view
        }
        let o = match k {
            KeyIn::Enter => return Outcome::Submit,
            KeyIn::Esc => return Outcome::Cancel,
            KeyIn::Backspace => self.edit(Edit::Backspace),
            KeyIn::ClearAll => self.edit(Edit::Clear),
            KeyIn::Tab => self.edit(Edit::ToggleReveal),
            KeyIn::Char(c) => self.edit(Edit::Insert(c)),
        };
        if was_picker != self.showing_picker() {
            Outcome::Redraw
        } else {
            o
        }
    }

    /// The screen body between the status bar and the hints bar.
    /// `heading`: e.g. the network name; `done`: label of the Done cell
    /// ("Join"); `kb_help`: lines for the keyboard view.
    pub fn draw<D: Canvas>(&self, d: &mut D, heading: &str, label: &str, done: &str, kb_help: &[&str], note: EntryNote) {
        if !self.showing_picker() {
            draw_text_entry(d, heading, label, &self.field, kb_help, note);
            return;
        }
        let x0 = 10;
        fill(d, 0, STATUS_H, 320, (240 - STATUS_H - 16) as u32, BG);
        text::draw_fit(d, Font::BodyBold, heading, x0, STATUS_H + 4, 230, FG);
        let count = alloc::format!("{} {}/{}", label, self.field.len(), self.field.max_len);
        text::draw_aligned(d, Font::Tiny, &count, 310, STATUS_H + 7, Align::Right, FAINT);
        let border = match note {
            EntryNote::Error(_) => WARN,
            EntryNote::Ok(_) => OK,
            _ => SEL,
        };
        draw_field(d, &self.field, x0, STATUS_H + 22, 300, 28, border);
        let (msg, c) = match note {
            EntryNote::None => (None, DIM),
            EntryNote::Info(m) => (Some(m), DIM),
            EntryNote::Error(m) => (Some(m), WARN),
            EntryNote::Busy(m) => (Some(m), ACCENT),
            EntryNote::Ok(m) => (Some(m), OK),
        };
        if let Some(m) = msg {
            text::draw_fit(d, Font::Small, m, x0, STATUS_H + 53, 300, c);
        }
        let p = &self.pick;
        for r in 0..p.rows() {
            for col in 0..p.row_len(r) {
                let (x, y, w, h) = p.cell_rect(r, col);
                let sel = r == p.row && col == p.col;
                let cell = p.cell(r, col);
                let tab_now = matches!(cell, Some(Cell::Tab(i)) if i == p.set);
                let bg = if sel {
                    SEL
                } else if matches!(cell, Some(Cell::Done)) {
                    rgb(0, 70, 50)
                } else {
                    PANEL
                };
                let _ = RoundedRectangle::with_equal_corners(rect(x, y, w, h), Size::new(5, 5)).into_styled(PrimitiveStyle::with_fill(bg)).draw(d);
                if tab_now && !sel {
                    let _ = RoundedRectangle::with_equal_corners(rect(x, y, w, h), Size::new(5, 5)).into_styled(PrimitiveStyle::with_stroke(ACCENT, 1)).draw(d);
                }
                let cx = x + w as i32 / 2;
                let cy = y + h as i32 / 2;
                match cell {
                    Some(Cell::Char(ch)) => text::draw_glyph_centered(d, Font::Large, ch, cx, cy, FG),
                    Some(other) => {
                        let s = match other {
                            Cell::Tab(i) => SETS[i].0,
                            Cell::Space => "space",
                            Cell::Delete => "del",
                            Cell::Keyboard => "kbd",
                            _ => done,
                        };
                        let col = if tab_now { ACCENT } else { FG };
                        text::draw_aligned(d, Font::Small, s, cx, cy - 6, Align::Center, col);
                    }
                    None => {}
                }
            }
        }
    }

    /// Hints for the bar under the grid / keyboard view.
    pub fn hints(&self) -> [(&'static str, &'static str); 4] {
        let show = if self.field.reveal { "hide" } else { "show" };
        if self.showing_picker() {
            [("OK", "pick"), ("X", if self.field.is_empty() { "cancel" } else { "delete" }), ("Sq", "abc/ABC/123"), ("Tri", show)]
        } else {
            [("OK", "done"), ("Tri", show), ("X", "cancel"), ("Arrows", "pick on screen")]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::String;

    /// Type `s` with the buttons only (shortest path to each cell, then OK).
    fn pad_type(e: &mut TextEntry, s: &str) {
        for ch in s.chars() {
            let target = if ch == ' ' { Cell::Space } else { Cell::Char(ch) };
            let path = e.pick.presses_to(target).expect("reachable");
            assert!(path.len() <= 12, "{ch:?} takes {} presses", path.len());
            for p in path {
                e.pad(p);
            }
            assert_eq!(e.pick.selected(), Some(target));
            assert_eq!(e.pad(Pad::Select), Outcome::Redraw, "{ch:?}");
        }
    }

    fn goto_action(e: &mut TextEntry, a: Cell) {
        while e.pick.row != e.pick.char_rows() {
            e.pad(Pad::Up);
        }
        let mut n = 0;
        while e.pick.selected() != Some(a) {
            e.pad(Pad::Right);
            n += 1;
            assert!(n < 20, "{a:?} not in the action row");
        }
    }

    #[test]
    fn every_printable_ascii_is_reachable_once() {
        let mut all: Vec<char> = SETS.iter().flat_map(|(_, s)| s.chars()).collect();
        all.push(' '); // the Space cell
        let n = all.len();
        all.sort();
        all.dedup();
        assert_eq!(all.len(), n, "no character twice");
        let want: Vec<char> = (' '..='~').collect();
        assert_eq!(all, want, "exactly 0x20..=0x7E");
        assert_eq!((LOWER.len(), UPPER.len(), DIGITS_SYM.len(), MORE_SYM.len()), (26, 26, 30, 12));
        assert!(SETS.iter().all(|(_, s)| s.len().div_ceil(COLS) <= MAX_CHAR_ROWS));
    }

    #[test]
    fn grid_fits_320x240() {
        let mut p = Picker::default();
        for off in [false, true] {
            p.offer_keyboard = off;
            for s in 0..SETS.len() {
                p.set = s;
                for r in 0..p.rows() {
                    for c in 0..p.row_len(r) {
                        let (x, y, w, h) = p.cell_rect(r, c);
                        assert!(x >= 0 && x + w as i32 <= 320 && y + h as i32 <= 240 - 16 - 4, "set {s} cell {r},{c}");
                    }
                }
            }
        }
    }

    #[test]
    fn navigation_wraps_and_keeps_the_column() {
        let mut e = TextEntry::wifi_password();
        assert!(e.showing_picker(), "no keyboard: grid");
        assert_eq!(e.pick.selected(), Some(Cell::Char('a')));
        e.pad(Pad::Left);
        assert_eq!(e.pick.selected(), Some(Cell::Char('j')), "wraps to the row end");
        e.pad(Pad::Right);
        e.pad(Pad::Up);
        assert_eq!(e.pick.row, e.pick.char_rows(), "up from the top = action row");
        assert_eq!(e.pick.selected(), Some(Cell::Tab(0)));
        e.pad(Pad::Down);
        assert_eq!(e.pick.selected(), Some(Cell::Char('a')), "down from the action row = top");
        // column kept: 'e' down twice = 'y'? row 2 has u..z (6): nearest to 'e' column
        for _ in 0..4 {
            e.pad(Pad::Right);
        }
        e.pad(Pad::Down);
        assert_eq!(e.pick.selected(), Some(Cell::Char('o')));
        e.pad(Pad::Down);
        assert_eq!(e.pick.selected(), Some(Cell::Char('y')));
        // short row -> nearest column in the next set
        e.pick.col = 5; // 'z'
        e.pad(Pad::NextSet);
        assert_eq!(e.pick.selected(), Some(Cell::Char('Z')));
        e.pad(Pad::NextSet);
        e.pad(Pad::NextSet);
        assert_eq!(e.pick.set, 3);
        assert!(e.pick.row < e.pick.char_rows(), "clamped into the 2-row set");
        e.pad(Pad::NextSet);
        assert_eq!(e.pick.set, 0, "Square cycles");
    }

    #[test]
    fn types_letters_digits_symbols_and_space_with_buttons() {
        let mut e = TextEntry::wifi_password();
        let pw = "Zq9 !\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~xA";
        pad_type(&mut e, pw);
        assert_eq!(e.field.as_str(), pw);
        assert!(e.field.masked && !e.field.reveal, "masked by default");
    }

    #[test]
    fn editing_delete_space_reveal_done() {
        let mut e = TextEntry::wifi_password();
        pad_type(&mut e, "ab");
        goto_action(&mut e, Cell::Delete);
        assert_eq!(e.pad(Pad::Select), Outcome::Redraw);
        assert_eq!(e.field.as_str(), "a");
        assert_eq!(e.pad(Pad::Reveal), Outcome::Redraw);
        assert!(e.field.reveal);
        e.pad(Pad::Reveal);
        assert!(!e.field.reveal);
        goto_action(&mut e, Cell::Tab(2));
        e.pad(Pad::Select);
        assert_eq!(e.pick.set, 2, "tab cell switches the set and stays on the tab");
        assert_eq!(e.pick.selected(), Some(Cell::Tab(2)));
        goto_action(&mut e, Cell::Done);
        assert_eq!(e.pad(Pad::Select), Outcome::Submit);
        assert_eq!(e.field.take(), "a");
        assert!(e.field.is_empty());
    }

    #[test]
    fn x_deletes_then_cancels() {
        let mut e = TextEntry::wifi_password();
        pad_type(&mut e, "xy");
        assert_eq!(e.pad(Pad::Back), Outcome::Redraw);
        assert_eq!(e.pad(Pad::Back), Outcome::Redraw);
        assert!(e.field.is_empty());
        assert_eq!(e.pad(Pad::Back), Outcome::Cancel);
    }

    #[test]
    fn max_length_and_ascii_only() {
        let mut e = TextEntry::wifi_password();
        for _ in 0..70 {
            e.pad(Pad::Select);
        }
        assert_eq!(e.field.len(), 63);
        assert_eq!(e.pad(Pad::Select), Outcome::None, "full");
        let mut k = TextEntry::wifi_password();
        k.set_keyboard(true);
        assert_eq!(k.key(KeyIn::Char('é')), Outcome::None);
        assert_eq!(k.key(KeyIn::Char('\t')), Outcome::None);
    }

    #[test]
    fn keyboard_view_then_hotplug_fallback() {
        let mut e = TextEntry::wifi_password();
        assert!(e.set_keyboard(true));
        assert!(!e.showing_picker(), "keyboard plugged in: keyboard view");
        for c in "Pa ss".chars() {
            assert_eq!(e.key(KeyIn::Char(c)), Outcome::Redraw);
        }
        assert_eq!(e.key(KeyIn::Backspace), Outcome::Redraw);
        assert_eq!(e.key(KeyIn::Tab), Outcome::Redraw);
        assert!(e.field.reveal);
        e.key(KeyIn::Tab);
        // OK = done in the keyboard view; X = cancel
        assert_eq!(e.pad(Pad::Select), Outcome::Submit);
        assert_eq!(e.pad(Pad::Back), Outcome::Cancel);
        // an arrow brings up the grid without changing the text
        assert_eq!(e.pad(Pad::Down), Outcome::Redraw);
        assert!(e.showing_picker());
        // typing returns to the keyboard view
        assert_eq!(e.key(KeyIn::Char('!')), Outcome::Redraw);
        assert!(!e.showing_picker());
        // unplugged mid-entry: grid, text kept, buttons keep working
        assert!(e.set_keyboard(false));
        assert!(!e.set_keyboard(false), "no change, no redraw");
        assert!(e.showing_picker());
        assert_eq!(e.field.as_str(), "Pa s!");
        pad_type(&mut e, "1");
        assert_eq!(e.field.as_str(), "Pa s!1");
        assert_eq!(e.key(KeyIn::ClearAll), Outcome::Redraw);
        assert!(e.field.is_empty());
        assert_eq!(e.key(KeyIn::Enter), Outcome::Submit);
        assert_eq!(e.key(KeyIn::Esc), Outcome::Cancel);
    }

    #[test]
    fn keyboard_cell_only_when_offered() {
        let mut e = TextEntry::wifi_password();
        assert!(!e.pick.action_cells().iter().any(|(c, _)| *c == Cell::Keyboard));
        e.pick.set_offer_keyboard(true);
        goto_action(&mut e, Cell::Keyboard);
        assert_eq!(e.pad(Pad::Select), Outcome::WantKeyboard);
        // the offer goes away (keyboard arrived): selection stays valid
        e.pick.set_offer_keyboard(false);
        assert!(e.pick.selected().is_some());
        assert_ne!(e.pick.selected(), Some(Cell::Keyboard));
    }

    #[test]
    fn secrets_stay_out_of_debug_and_outcomes() {
        let mut e = TextEntry::wifi_password();
        pad_type(&mut e, "s3cr3t-pw");
        let dbg = alloc::format!("{e:?} {:?} {:?}", e.pad(Pad::Up), e.pick);
        assert!(!dbg.contains("s3cr3t"), "{dbg}");
        assert!(dbg.contains("redacted"));
        let t: String = e.field.take();
        assert_eq!(t, "s3cr3t-pw");
        assert!(e.field.is_empty());
    }

    #[test]
    fn draws_both_views_masked() {
        use crate::fb::Framebuffer;
        let mut fb = Framebuffer::default();
        let mut e = TextEntry::wifi_password();
        pad_type(&mut e, "abc");
        e.draw(&mut fb, "Network", "Password", "Join", &["help"], EntryNote::Info("No keyboard"));
        // three dots on the field's centre line
        let y = (STATUS_H + 22 + 14) as usize;
        let fg = embedded_graphics::pixelcolor::raw::RawU16::from(FG).into_inner();
        let mut runs = 0;
        let mut inrun = false;
        for x in 12..290 {
            let on = fb.get(x, y) == fg;
            if on && !inrun {
                runs += 1;
            }
            inrun = on;
        }
        assert_eq!(runs, 3);
        e.set_keyboard(true);
        e.draw(&mut fb, "Network", "Password", "Join", &["help"], EntryNote::None);
        assert_eq!(e.hints()[0], ("OK", "done"));
    }
}
