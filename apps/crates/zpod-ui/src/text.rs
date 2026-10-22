//! Text on u8g2 fonts (Latin-1 coverage via the `_tf` variants; unknown
//! glyphs are skipped instead of failing).
use alloc::string::String;
use embedded_graphics::{pixelcolor::Rgb565, prelude::*};
use u8g2_fonts::{
    fonts,
    types::{FontColor, HorizontalAlignment, VerticalPosition},
    FontRenderer,
};

use crate::Canvas;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Font {
    /// 8 px Helvetica (status bar, fine print)
    Tiny,
    /// 10 px
    Small,
    /// 12 px (list rows)
    Body,
    BodyBold,
    /// 14 px bold (titles)
    Title,
    /// 18 px bold
    Large,
    /// 24 px bold
    Huge,
    /// Logisoso digits (clock, scores)
    Digits20,
    Digits32,
    Digits58,
    Digits92,
    /// Open Iconic "play" glyphs (16 px)
    IconPlay,
    /// Open Iconic "embedded" glyphs (16 px)
    IconEmbedded,
}

macro_rules! r {
    ($f:ident) => {
        FontRenderer::new::<fonts::$f>().with_ignore_unknown_chars(true)
    };
}

static TINY: FontRenderer = r!(u8g2_font_helvR08_tf);
static SMALL: FontRenderer = r!(u8g2_font_helvR10_tf);
static BODY: FontRenderer = r!(u8g2_font_helvR12_tf);
static BODYB: FontRenderer = r!(u8g2_font_helvB12_tf);
static TITLE: FontRenderer = r!(u8g2_font_helvB14_tf);
static LARGE: FontRenderer = r!(u8g2_font_helvB18_tf);
static HUGE: FontRenderer = r!(u8g2_font_helvB24_tf);
static D20: FontRenderer = r!(u8g2_font_logisoso20_tf);
static D32: FontRenderer = r!(u8g2_font_logisoso32_tf);
static D58: FontRenderer = r!(u8g2_font_logisoso58_tf);
static D92: FontRenderer = r!(u8g2_font_logisoso92_tn);
static IPLAY: FontRenderer = r!(u8g2_font_open_iconic_play_2x_t);
static IEMB: FontRenderer = r!(u8g2_font_open_iconic_embedded_2x_t);

impl Font {
    pub fn renderer(self) -> &'static FontRenderer {
        match self {
            Font::Tiny => &TINY,
            Font::Small => &SMALL,
            Font::Body => &BODY,
            Font::BodyBold => &BODYB,
            Font::Title => &TITLE,
            Font::Large => &LARGE,
            Font::Huge => &HUGE,
            Font::Digits20 => &D20,
            Font::Digits32 => &D32,
            Font::Digits58 => &D58,
            Font::Digits92 => &D92,
            Font::IconPlay => &IPLAY,
            Font::IconEmbedded => &IEMB,
        }
    }

    /// Ascent above baseline in px (cap height-ish).
    pub fn ascent(self) -> i32 {
        self.renderer().get_ascent() as i32
    }
    pub fn line_height(self) -> i32 {
        self.renderer().get_default_line_height() as i32
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// Width of `s` in pixels (advance).
pub fn width(font: Font, s: &str) -> i32 {
    font.renderer()
        .get_rendered_dimensions(s, Point::zero(), VerticalPosition::Baseline)
        .map(|d| d.advance.x)
        .unwrap_or(0)
}

/// Draw with the top of the font's ascent at `y`. Returns the advance width.
pub fn draw<D: Canvas>(d: &mut D, font: Font, s: &str, x: i32, y: i32, color: Rgb565) -> i32 {
    font.renderer()
        .render(s, Point::new(x, y), VerticalPosition::Top, FontColor::Transparent(color), d)
        .map(|r| r.advance.x)
        .unwrap_or(0)
}

/// Draw with baseline at `y`.
pub fn draw_baseline<D: Canvas>(d: &mut D, font: Font, s: &str, x: i32, y: i32, color: Rgb565) -> i32 {
    font.renderer()
        .render(s, Point::new(x, y), VerticalPosition::Baseline, FontColor::Transparent(color), d)
        .map(|r| r.advance.x)
        .unwrap_or(0)
}

/// Horizontally aligned draw (top at `y`).
pub fn draw_aligned<D: Canvas>(d: &mut D, font: Font, s: &str, x: i32, y: i32, align: Align, color: Rgb565) {
    let w = width(font, s);
    let x0 = match align {
        Align::Left => x,
        Align::Center => x - w / 2,
        Align::Right => x - w,
    };
    draw(d, font, s, x0, y, color);
}

/// Shorten `s` with "..." so it fits in `max_w` pixels.
pub fn fit(font: Font, s: &str, max_w: i32) -> String {
    if width(font, s) <= max_w {
        return String::from(s);
    }
    let dots = width(font, "...");
    let mut out = String::new();
    let mut w = 0;
    for ch in s.chars() {
        let mut buf = [0u8; 4];
        let cw = width(font, ch.encode_utf8(&mut buf));
        if w + cw + dots > max_w {
            break;
        }
        w += cw;
        out.push(ch);
    }
    out.push_str("...");
    out
}

/// Draw `s` fitted into `max_w`.
pub fn draw_fit<D: Canvas>(d: &mut D, font: Font, s: &str, x: i32, y: i32, max_w: i32, color: Rgb565) {
    let t = fit(font, s, max_w);
    draw(d, font, &t, x, y, color);
}

/// Center-aligned helper exposing u8g2's own alignment (used by icons).
pub fn draw_glyph_centered<D: Canvas>(d: &mut D, font: Font, ch: char, cx: i32, cy: i32, color: Rgb565) {
    let mut b = [0u8; 4];
    let _ = font.renderer().render_aligned(
        &*ch.encode_utf8(&mut b),
        Point::new(cx, cy),
        VerticalPosition::Center,
        HorizontalAlignment::Center,
        FontColor::Transparent(color),
        d,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fit_shortens() {
        let s = "A very long album title that will never fit";
        let f = fit(Font::Body, s, 100);
        assert!(f.ends_with("..."));
        assert!(width(Font::Body, &f) <= 100);
        assert_eq!(fit(Font::Body, "Short", 100), "Short");
    }
    #[test]
    fn latin1_has_width() {
        assert!(width(Font::Body, "Björk Guðmundsdóttir") > 50);
    }
}
