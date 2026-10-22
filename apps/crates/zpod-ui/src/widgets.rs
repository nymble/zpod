//! Lists, dialogs, toasts, progress bars, button hints.
use embedded_graphics::{
    pixelcolor::Rgb565,
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle, RoundedRectangle},
};

use crate::{fill, image::Image565, rect, text, theme::*, Canvas, Font};

/// Selection + scroll state for a vertical list.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ListState {
    pub sel: usize,
    pub top: usize,
}

impl ListState {
    /// Move selection by `delta` (wrapping at the ends) and keep it visible.
    pub fn step(&mut self, delta: i32, len: usize, visible: usize) {
        if len == 0 {
            self.sel = 0;
            self.top = 0;
            return;
        }
        let n = len as i32;
        self.sel = (((self.sel as i32 + delta) % n + n) % n) as usize;
        self.scroll_into_view(len, visible);
    }

    /// Page jump without wrapping.
    pub fn page(&mut self, delta: i32, len: usize, visible: usize) {
        if len == 0 {
            return;
        }
        self.sel = (self.sel as i32 + delta).clamp(0, len as i32 - 1) as usize;
        self.scroll_into_view(len, visible);
    }

    pub fn scroll_into_view(&mut self, len: usize, visible: usize) {
        if self.sel >= len {
            self.sel = len.saturating_sub(1);
        }
        if self.sel < self.top {
            self.top = self.sel;
        } else if visible > 0 && self.sel >= self.top + visible {
            self.top = self.sel + 1 - visible;
        }
        let max_top = len.saturating_sub(visible);
        if self.top > max_top {
            self.top = max_top;
        }
    }
}

pub struct Row<'a> {
    pub title: &'a str,
    pub sub: Option<&'a str>,
    pub right: Option<&'a str>,
    pub icon: Option<&'a Image565>,
    /// Small coloured marker left of the title (e.g. "now playing").
    pub mark: Option<Rgb565>,
}

impl<'a> Row<'a> {
    pub fn simple(title: &'a str) -> Self {
        Row { title, sub: None, right: None, icon: None, mark: None }
    }
}

/// Draw rows [top, top+visible) of a list into `area`. `row` builds a row.
pub fn draw_list<'a, D: Canvas>(
    d: &mut D,
    area: Rectangle,
    row_h: i32,
    st: &ListState,
    len: usize,
    mut row: impl FnMut(usize) -> Row<'a>,
) {
    let x = area.top_left.x;
    let y0 = area.top_left.y;
    let w = area.size.width as i32;
    let h = area.size.height as i32;
    let _ = d.fill_solid(&area, BG);
    let visible = (h / row_h) as usize;
    let scroll = len > visible;
    let tw = if scroll { w - 6 } else { w };
    for i in 0..visible {
        let idx = st.top + i;
        if idx >= len {
            break;
        }
        let y = y0 + i as i32 * row_h;
        let selected = idx == st.sel;
        if selected {
            let _ = RoundedRectangle::with_equal_corners(rect(x + 2, y + 1, (tw - 4) as u32, (row_h - 2) as u32), Size::new(4, 4))
                .into_styled(PrimitiveStyle::with_fill(SEL))
                .draw(d);
        }
        let r = row(idx);
        let mut tx = x + 10;
        if let Some(ic) = r.icon {
            ic.draw(d, tx - 2, y + (row_h - ic.h as i32) / 2);
            tx += ic.w as i32 + 6;
        }
        if let Some(m) = r.mark {
            fill(d, x + 4, y + 6, 3, (row_h - 12) as u32, m);
        }
        let mut right_w = 0;
        if let Some(rt) = r.right {
            right_w = text::width(Font::Small, rt) + 10;
            text::draw(d, Font::Small, rt, x + tw - right_w, y + (row_h - 10) / 2, if selected { FG } else { DIM });
        }
        let maxw = x + tw - right_w - tx - 6;
        match r.sub {
            Some(sub) if row_h >= 34 => {
                text::draw_fit(d, Font::BodyBold, r.title, tx, y + 4, maxw, FG);
                text::draw_fit(d, Font::Small, sub, tx, y + row_h - 15, maxw, if selected { FG } else { DIM });
            }
            _ => {
                text::draw_fit(d, Font::Body, r.title, tx, y + (row_h - 12) / 2, maxw, FG);
            }
        }
    }
    if scroll && visible > 0 {
        let track_h = h - 4;
        let thumb = ((visible * track_h as usize) / len).max(8) as i32;
        let pos = if len > visible { (st.top * (track_h - thumb) as usize / (len - visible)) as i32 } else { 0 };
        fill(d, x + w - 4, y0 + 2, 2, track_h as u32, FAINT);
        fill(d, x + w - 5, y0 + 2 + pos, 4, thumb as u32, DIM);
    }
    if len == 0 {
        text::draw_aligned(d, Font::Body, "(empty)", x + w / 2, y0 + h / 2 - 6, text::Align::Center, DIM);
    }
}

pub fn progress_bar<D: Canvas>(d: &mut D, x: i32, y: i32, w: u32, h: u32, frac: f32, fg: Rgb565, bg: Rgb565) {
    fill(d, x, y, w, h, bg);
    let fw = (frac.clamp(0.0, 1.0) * w as f32) as u32;
    if fw > 0 {
        fill(d, x, y, fw, h, fg);
    }
}

/// Centered overlay box with one line of large text.
pub fn toast<D: Canvas>(d: &mut D, msg: &str) {
    let w = (text::width(Font::Large, msg) + 40).clamp(120, 300);
    let x = (320 - w) / 2;
    let r = rect(x, 96, w as u32, 48);
    let _ = RoundedRectangle::with_equal_corners(r, Size::new(8, 8))
        .into_styled(PrimitiveStyle::with_fill(PANEL))
        .draw(d);
    let _ = RoundedRectangle::with_equal_corners(r, Size::new(8, 8))
        .into_styled(PrimitiveStyle::with_stroke(ACCENT, 2))
        .draw(d);
    text::draw_aligned(d, Font::Large, msg, 160, 110, text::Align::Center, ACCENT);
}

/// Toast area (to restore what was under it).
pub const TOAST_AREA: (i32, i32, u32, u32) = (10, 96, 300, 48);

/// Where the side-button volume overlay is drawn (x, y, w, h). The same
/// box on every screen and in every app (drawn by the platform layer).
pub const VOLUME_OVERLAY: (i32, i32, u32, u32) = (50, 88, 220, 64);

/// The one volume overlay: speaker, "Volume 46%", a bar with the 80 % cap.
pub fn volume_overlay<D: Canvas>(d: &mut D, pct: Option<u8>, cap: u8) {
    let (x, y, w, h) = VOLUME_OVERLAY;
    // inset by 2 px: the 2-px stroke must stay inside VOLUME_OVERLAY, which
    // is the area the platform layer saves and restores
    let r = rect(x + 2, y + 2, w - 4, h - 4);
    let _ = RoundedRectangle::with_equal_corners(r, Size::new(8, 8))
        .into_styled(PrimitiveStyle::with_fill(PANEL))
        .draw(d);
    let _ = RoundedRectangle::with_equal_corners(r, Size::new(8, 8))
        .into_styled(PrimitiveStyle::with_stroke(ACCENT, 2))
        .draw(d);
    crate::status::speaker_icon(d, x + 14, y + 12, ACCENT);
    let label = match pct {
        Some(p) => alloc::format!("Volume {p}%"),
        None => alloc::string::String::from("Volume --"),
    };
    text::draw(d, Font::Large, &label, x + 34, y + 8, ACCENT);
    let (bx, by, bw, bh) = (x + 14, y + 40, w - 28, 12u32);
    fill(d, bx, by, bw, bh, BAR);
    let cap = cap.clamp(1, 100) as u32;
    if let Some(p) = pct {
        let fw = (bw * (p as u32).min(cap) / cap).min(bw);
        if fw > 0 {
            fill(d, bx, by, fw, bh, OK);
        }
    }
    let _ = Rectangle::new(Point::new(bx - 1, by - 1), Size::new(bw + 2, bh + 2))
        .into_styled(PrimitiveStyle::with_stroke(FG, 1))
        .draw(d);
}

/// Confirmation dialog: title + lines; OK/X hints drawn by caller via hints().
pub fn dialog<D: Canvas>(d: &mut D, title: &str, lines: &[&str], title_color: Rgb565) {
    let r = rect(16, 44, 288, 150);
    let _ = RoundedRectangle::with_equal_corners(r, Size::new(10, 10))
        .into_styled(PrimitiveStyle::with_fill(PANEL))
        .draw(d);
    let _ = RoundedRectangle::with_equal_corners(r, Size::new(10, 10))
        .into_styled(PrimitiveStyle::with_stroke(title_color, 2))
        .draw(d);
    text::draw_aligned(d, Font::Huge, title, 160, 58, text::Align::Center, title_color);
    let mut y = 100;
    for l in lines {
        text::draw_aligned(d, Font::Body, l, 160, y, text::Align::Center, FG);
        y += 22;
    }
}

/// Bottom hint line: pairs of (button glyph label, action), e.g. ("OK", "play").
pub fn hints<D: Canvas>(d: &mut D, items: &[(&str, &str)]) {
    let y = 240 - 16;
    fill(d, 0, y, 320, 16, BAR);
    let mut x = 6;
    for (b, a) in items {
        let bw = text::width(Font::Tiny, b) + 6;
        let _ = RoundedRectangle::with_equal_corners(rect(x, y + 2, bw as u32, 12), Size::new(3, 3))
            .into_styled(PrimitiveStyle::with_fill(FAINT))
            .draw(d);
        text::draw(d, Font::Tiny, b, x + 3, y + 4, WHITE);
        x += bw + 3;
        x += text::draw(d, Font::Tiny, a, x, y + 4, DIM) + 9;
    }
}

pub const HINTS_H: i32 = 16;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn list_scrolls_and_wraps() {
        let mut s = ListState::default();
        s.step(-1, 10, 4);
        assert_eq!((s.sel, s.top), (9, 6));
        s.step(1, 10, 4);
        assert_eq!((s.sel, s.top), (0, 0));
        for _ in 0..5 {
            s.step(1, 10, 4);
        }
        assert_eq!((s.sel, s.top), (5, 2));
        s.page(100, 10, 4);
        assert_eq!((s.sel, s.top), (9, 6));
        s.scroll_into_view(3, 4);
        assert_eq!((s.sel, s.top), (2, 0));
    }
}
