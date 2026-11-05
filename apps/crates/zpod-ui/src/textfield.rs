//! Keyboard text entry: [`TextField`] state + [`draw_text_entry`] screen.
//!
//! Made for secrets first (Wi-Fi passwords): a masked field shows dots, its
//! `Debug` never prints the content, and the buffer is overwritten with
//! zeros when cleared, taken or dropped. Input comes from a USB/BT keyboard
//! (zpod-input); buttons-only entry is a later step.

use alloc::string::String;
use alloc::vec::Vec;
use embedded_graphics::{
    pixelcolor::Rgb565,
    prelude::*,
    primitives::{Circle, PrimitiveStyle, RoundedRectangle},
};

use crate::{fill, rect, text, text::Align, theme::*, Canvas, Font};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edit {
    Insert(char),
    Backspace,
    /// Delete everything.
    Clear,
    /// Show / hide a masked field's characters.
    ToggleReveal,
}

pub struct TextField {
    buf: Vec<u8>,
    pub masked: bool,
    /// Reveal a masked field (user asked to see it).
    pub reveal: bool,
    pub max_len: usize,
    /// Only printable ASCII (WPA passphrases are 8-63 ASCII characters).
    pub ascii_only: bool,
}

impl core::fmt::Debug for TextField {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TextField")
            .field("len", &self.len())
            .field("masked", &self.masked)
            .field("text", &if self.masked { "<redacted>" } else { "<text>" })
            .finish()
    }
}

fn wipe(v: &mut Vec<u8>) {
    for b in v.iter_mut() {
        // volatile so the zeroing is not optimised away
        unsafe { core::ptr::write_volatile(b, 0) };
    }
    v.clear();
}

impl TextField {
    pub fn new(masked: bool, max_len: usize) -> Self {
        TextField { buf: Vec::with_capacity(max_len * 4), masked, reveal: false, max_len, ascii_only: false }
    }

    /// Masked, printable ASCII, up to 63 characters.
    pub fn wifi_password() -> Self {
        let mut f = TextField::new(true, 63);
        f.ascii_only = true;
        f
    }

    pub fn len(&self) -> usize {
        self.as_str().chars().count()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// The content (for submitting; never log it).
    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf).unwrap_or("")
    }

    /// Returns true when the field changed.
    pub fn apply(&mut self, e: Edit) -> bool {
        match e {
            Edit::Insert(c) => {
                if c.is_control() || (self.ascii_only && !(' '..='~').contains(&c)) || self.len() >= self.max_len {
                    return false;
                }
                // grow without leaving old copies behind in a freed allocation
                if self.buf.len() + 4 > self.buf.capacity() {
                    let mut n = Vec::with_capacity((self.buf.capacity() * 2).max(16));
                    n.extend_from_slice(&self.buf);
                    wipe(&mut self.buf);
                    self.buf = n;
                }
                let mut tmp = [0u8; 4];
                self.buf.extend_from_slice(c.encode_utf8(&mut tmp).as_bytes());
                true
            }
            Edit::Backspace => {
                let Some((i, _)) = self.as_str().char_indices().last() else { return false };
                for b in self.buf[i..].iter_mut() {
                    unsafe { core::ptr::write_volatile(b, 0) };
                }
                self.buf.truncate(i);
                true
            }
            Edit::Clear => {
                let had = !self.buf.is_empty();
                wipe(&mut self.buf);
                had
            }
            Edit::ToggleReveal => {
                if !self.masked {
                    return false;
                }
                self.reveal = !self.reveal;
                true
            }
        }
    }

    /// Move the content out and wipe the field.
    pub fn take(&mut self) -> String {
        let s = String::from(self.as_str());
        wipe(&mut self.buf);
        self.reveal = false;
        s
    }

    /// What the screen shows: dots count or the (tail of the) text.
    fn shown(&self) -> Shown<'_> {
        if self.masked && !self.reveal {
            Shown::Dots(self.len())
        } else {
            Shown::Text(self.as_str())
        }
    }
}

impl Drop for TextField {
    fn drop(&mut self) {
        wipe(&mut self.buf);
    }
}

enum Shown<'a> {
    Dots(usize),
    Text(&'a str),
}

/// State line under the field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryNote<'a> {
    None,
    Info(&'a str),
    Error(&'a str),
    Busy(&'a str),
    Ok(&'a str),
}

pub const FIELD_X: i32 = 12;
pub const FIELD_W: u32 = 296;
pub const FIELD_H: u32 = 34;

/// Text entry screen body (below the status bar, above the hints):
/// `heading` (e.g. the network name), `label` ("Password"), the field with a
/// caret, a character count, `help` lines and a note.
pub fn draw_text_entry<D: Canvas>(d: &mut D, heading: &str, label: &str, field: &TextField, help: &[&str], note: EntryNote) {
    fill(d, 0, STATUS_H, 320, (240 - STATUS_H - 16) as u32, BG);
    text::draw_fit(d, Font::Large, heading, FIELD_X, STATUS_H + 10, 296, FG);
    text::draw(d, Font::Small, label, FIELD_X, STATUS_H + 40, DIM);
    let fy = STATUS_H + 56;
    let _ = RoundedRectangle::with_equal_corners(rect(FIELD_X, fy, FIELD_W, FIELD_H), Size::new(8, 8))
        .into_styled(PrimitiveStyle::with_fill(PANEL))
        .draw(d);
    let border = match note {
        EntryNote::Error(_) => WARN,
        EntryNote::Ok(_) => OK,
        _ => SEL,
    };
    let _ = RoundedRectangle::with_equal_corners(rect(FIELD_X, fy, FIELD_W, FIELD_H), Size::new(8, 8))
        .into_styled(PrimitiveStyle::with_stroke(border, 2))
        .draw(d);
    let inner_x = FIELD_X + 10;
    let inner_w = FIELD_W as i32 - 26;
    let caret_x = match field.shown() {
        Shown::Dots(n) => {
            let pitch = 10;
            let fit = (inner_w / pitch) as usize;
            let first = n.saturating_sub(fit);
            let mut x = inner_x;
            for _ in first..n {
                let _ = Circle::new(Point::new(x, fy + FIELD_H as i32 / 2 - 3), 7).into_styled(PrimitiveStyle::with_fill(FG)).draw(d);
                x += pitch;
            }
            x
        }
        Shown::Text(s) => {
            // show the tail that fits
            let mut start = 0;
            let chars: Vec<(usize, char)> = s.char_indices().collect();
            while start < chars.len() && text::width(Font::Body, &s[chars[start].0..]) > inner_w {
                start += 1;
            }
            let tail = if start < chars.len() { &s[chars[start].0..] } else { "" };
            inner_x + text::draw(d, Font::Body, tail, inner_x, fy + 10, FG)
        }
    };
    // caret
    fill(d, caret_x + 1, fy + 8, 2, FIELD_H - 16, ACCENT);
    let count = alloc::format!("{} / {}", field.len(), field.max_len);
    text::draw_aligned(d, Font::Tiny, &count, FIELD_X + FIELD_W as i32, fy + FIELD_H as i32 + 4, Align::Right, FAINT);
    let mut y = fy + FIELD_H as i32 + 18;
    let (msg, c): (Option<&str>, Rgb565) = match note {
        EntryNote::None => (None, DIM),
        EntryNote::Info(m) => (Some(m), DIM),
        EntryNote::Error(m) => (Some(m), WARN),
        EntryNote::Busy(m) => (Some(m), ACCENT),
        EntryNote::Ok(m) => (Some(m), OK),
    };
    if let Some(m) = msg {
        text::draw_fit(d, Font::BodyBold, m, FIELD_X, y, 296, c);
        y += 20;
    }
    for h in help {
        text::draw_fit(d, Font::Small, h, FIELD_X, y, 296, DIM);
        y += 15;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_and_limits() {
        let mut f = TextField::wifi_password();
        assert!(f.apply(Edit::Insert('a')));
        assert!(f.apply(Edit::Insert('B')));
        assert!(!f.apply(Edit::Insert('\n')), "control chars refused");
        assert!(!f.apply(Edit::Insert('é')), "ASCII only for WPA");
        assert_eq!(f.as_str(), "aB");
        assert!(f.apply(Edit::Backspace));
        assert_eq!(f.as_str(), "a");
        for _ in 0..100 {
            f.apply(Edit::Insert('x'));
        }
        assert_eq!(f.len(), 63);
        assert!(f.apply(Edit::Clear));
        assert!(f.is_empty());
        assert!(!f.apply(Edit::Backspace));
    }

    #[test]
    fn unicode_backspace_when_allowed() {
        let mut f = TextField::new(false, 10);
        f.apply(Edit::Insert('é'));
        f.apply(Edit::Insert('z'));
        f.apply(Edit::Backspace);
        assert_eq!(f.as_str(), "é");
        f.apply(Edit::Backspace);
        assert_eq!(f.as_str(), "");
        assert!(!f.apply(Edit::ToggleReveal), "plain field has nothing to reveal");
    }

    #[test]
    fn debug_never_prints_secret() {
        let mut f = TextField::wifi_password();
        for c in "hunter22".chars() {
            f.apply(Edit::Insert(c));
        }
        let s = alloc::format!("{f:?}");
        assert!(!s.contains("hunter") && s.contains("redacted") && s.contains("len: 8"), "{s}");
        let t = f.take();
        assert_eq!(t, "hunter22");
        assert!(f.is_empty());
    }

    #[test]
    fn draws_dots_not_text() {
        use crate::fb::Framebuffer;
        let mut fb = Framebuffer::default();
        let mut f = TextField::wifi_password();
        for c in "abcdefgh".chars() {
            f.apply(Edit::Insert(c));
        }
        draw_text_entry(&mut fb, "Network", "Password", &f, &["Enter = join"], EntryNote::None);
        // masked: field row has exactly 8 dot blobs (count FG runs on the centre line)
        let y = (STATUS_H + 56 + FIELD_H as i32 / 2) as usize;
        let fg = embedded_graphics::pixelcolor::raw::RawU16::from(FG).into_inner();
        let mut runs = 0;
        let mut inrun = false;
        for x in (FIELD_X as usize + 2)..(FIELD_X as usize + FIELD_W as usize - 14) {
            let on = fb.get(x, y) == fg;
            if on && !inrun {
                runs += 1;
            }
            inrun = on;
        }
        assert_eq!(runs, 8);
    }
}
