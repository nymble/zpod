//! Status bar: title | play state, volume, USB, Wi-Fi, battery %, time.
use alloc::format;
use embedded_graphics::{
    pixelcolor::Rgb565,
    prelude::*,
    primitives::{PrimitiveStyle, Triangle},
};
use zpod_core::{PlayState, StatusInfo, WifiMode};

use crate::{fill, text, theme::*, Canvas, Font};

/// Redraws the whole bar (20 px). Cheap: ~6400 px, sent only when changed.
pub fn draw_status_bar<D: Canvas>(d: &mut D, title: &str, st: &StatusInfo) {
    let w = d.bounding_box().size.width as i32;
    fill(d, 0, 0, w as u32, STATUS_H as u32, BAR);
    fill(d, 0, STATUS_H - 1, w as u32, 1, FAINT);
    let mut x = w - 4;

    // time
    if let Some(t) = st.time {
        let s = format!("{:02}:{:02}", t.hour, t.minute);
        x -= text::width(Font::BodyBold, &s);
        text::draw(d, Font::BodyBold, &s, x, 4, FG);
        x -= 8;
    }
    // battery
    if let Some(p) = st.battery_pct {
        let s = format!("{:.0}%", p);
        let tw = text::width(Font::Small, &s);
        x -= tw;
        text::draw(d, Font::Small, &s, x, 5, FG);
        x -= 22;
        battery_icon(d, x, 5, p, st.charging);
        x -= 6;
    }
    // wifi: Off (struck), Client (signal bars, dim while not connected),
    // Hotspot (AP badge + number of connected devices)
    match st.wifi_mode {
        WifiMode::Unknown => {
            x -= 13;
            wifi_icon(d, x, 4, st.wifi);
        }
        WifiMode::Off => {
            x -= 13;
            wifi_icon(d, x, 4, false);
        }
        WifiMode::Client { bars, connected } => {
            x -= 13;
            wifi_bars_icon(d, x, 4, if connected { bars } else { 0 }, connected);
        }
        WifiMode::Hotspot { clients } => {
            let s = format!("{}", clients);
            let tw = text::width(Font::Small, &s);
            x -= tw;
            text::draw(d, Font::Small, &s, x, 5, FG);
            x -= 18;
            hotspot_icon(d, x, 4);
        }
    }
    x -= 5;
    // usb
    if st.usb {
        x -= 22;
        fill(d, x, 4, 22, 12, CYAN);
        text::draw(d, Font::Tiny, "USB", x + 2, 6, BAR);
        x -= 5;
    }
    // volume
    if let Some(v) = st.volume_pct {
        let s = format!("{}", v);
        let tw = text::width(Font::Small, &s);
        x -= tw;
        text::draw(d, Font::Small, &s, x, 5, DIM);
        x -= 13;
        speaker_icon(d, x, 5, DIM);
        x -= 5;
    }
    // play state
    match st.play {
        PlayState::Playing => {
            x -= 10;
            let _ = Triangle::new(Point::new(x, 4), Point::new(x, 15), Point::new(x + 9, 9))
                .into_styled(PrimitiveStyle::with_fill(OK))
                .draw(d);
        }
        PlayState::Paused => {
            x -= 10;
            fill(d, x, 4, 3, 11, ACCENT);
            fill(d, x + 6, 4, 3, 11, ACCENT);
        }
        PlayState::Stopped => {
            x -= 10;
            fill(d, x, 5, 9, 9, DIM);
        }
        PlayState::Unknown => {}
    }
    x -= 6;
    // title
    text::draw_fit(d, Font::Title, title, 6, 3, x - 6, ACCENT);
}

/// 6-step battery icon (0 = empty/shutdown zone, 1..=5 bars), see
/// [`zpod_core::volume::battery_level6`].
pub fn battery_icon<D: Canvas>(d: &mut D, x: i32, y: i32, pct: f32, charging: bool) {
    let level = zpod_core::volume::battery_level6(pct);
    let c = match level {
        0 | 1 => WARN,
        2 => ACCENT,
        _ => OK,
    };
    // outline 18x10 + nub
    let edge = if level == 0 { WARN } else { FG };
    fill(d, x, y, 18, 1, edge);
    fill(d, x, y + 9, 18, 1, edge);
    fill(d, x, y, 1, 10, edge);
    fill(d, x + 17, y, 1, 10, edge);
    fill(d, x + 18, y + 3, 2, 4, edge);
    // five 2-px bars with 1-px gaps inside the 14-px cavity
    for i in 0..level as i32 {
        fill(d, x + 2 + i * 3, y + 2, 2, 6, c);
    }
    if charging {
        let _ = Triangle::new(Point::new(x + 10, y + 1), Point::new(x + 6, y + 6), Point::new(x + 9, y + 6))
            .into_styled(PrimitiveStyle::with_fill(WHITE))
            .draw(d);
    }
}

pub fn wifi_icon<D: Canvas>(d: &mut D, x: i32, y: i32, on: bool) {
    let c = if on { FG } else { FAINT };
    for i in 0..4 {
        let h = 3 + i * 3;
        fill(d, x + i * 3, y + 12 - h, 2, h as u32, c);
    }
    if !on {
        // strike-through
        let _ = embedded_graphics::primitives::Line::new(Point::new(x - 1, y + 12), Point::new(x + 11, y))
            .into_styled(PrimitiveStyle::with_stroke(WARN, 1))
            .draw(d);
    }
}

pub fn speaker_icon<D: Canvas>(d: &mut D, x: i32, y: i32, c: Rgb565) {
    fill(d, x, y + 3, 3, 4, c);
    let _ = Triangle::new(Point::new(x + 2, y + 5), Point::new(x + 8, y - 1), Point::new(x + 8, y + 11))
        .into_styled(PrimitiveStyle::with_fill(c))
        .draw(d);
}

/// Client mode: four bars, `bars` of them lit (0..=4); unlit ones faint.
/// Not connected: all faint plus a small dot (searching).
pub fn wifi_bars_icon<D: Canvas>(d: &mut D, x: i32, y: i32, bars: u8, connected: bool) {
    for i in 0..4 {
        let h = 3 + i * 3;
        let c = if connected && (i as u8) < bars.max(1) { FG } else { FAINT };
        fill(d, x + i * 3, y + 12 - h, 2, h as u32, c);
    }
    if !connected {
        fill(d, x, y, 3, 3, ACCENT);
    }
}

/// Hotspot mode: an access-point glyph (mast with two arcs), cyan.
pub fn hotspot_icon<D: Canvas>(d: &mut D, x: i32, y: i32) {
    use embedded_graphics::primitives::Arc;
    use embedded_graphics::geometry::AngleUnit;
    let st = PrimitiveStyle::with_stroke(CYAN, 1);
    fill(d, x + 7, y + 4, 2, 9, CYAN);
    fill(d, x + 5, y + 12, 6, 1, CYAN);
    for dia in [7u32, 13] {
        let tl = Point::new(x + 8 - dia as i32 / 2, y + 4 - dia as i32 / 2);
        let _ = Arc::new(tl, dia, (-45.0f32).deg(), 90.0f32.deg()).into_styled(st).draw(d);
        let _ = Arc::new(tl, dia, 135.0f32.deg(), 90.0f32.deg()).into_styled(st).draw(d);
    }
}
