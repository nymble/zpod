//! Tidepool screen: four Euclidean rings (one per lane) + settings panel.

use embedded_graphics::{
    pixelcolor::Rgb565,
    prelude::*,
    primitives::{Circle, PrimitiveStyle, RoundedRectangle},
};
use zpod_core::StatusInfo;
use zpod_music_core::generate::{euclid_hit, scale_name, LANES};
use zpod_music_core::harmony::Root;
use zpod_ui::{fill, rect, status::draw_status_bar, text, text::Align, theme::*, Font, Framebuffer};

use crate::{Gen, LEVELS};

const SEA: Rgb565 = rgb(6, 18, 34);
const RING: Rgb565 = rgb(24, 52, 78);
const LANE_COL: [Rgb565; LANES] = [rgb(120, 110, 220), rgb(64, 200, 255), rgb(255, 196, 0), rgb(90, 230, 190)];
const CX: i32 = 108;
const CY: i32 = 132;
const RADII: [i32; LANES] = [92, 72, 52, 32];

fn dot(fb: &mut Framebuffer, cx: i32, cy: i32, d: u32, c: Rgb565) {
    let _ = Circle::with_center(Point::new(cx, cy), d).into_styled(PrimitiveStyle::with_fill(c)).draw(fb);
}
fn ring(fb: &mut Framebuffer, r: i32, c: Rgb565) {
    let _ = Circle::with_center(Point::new(CX, CY), (2 * r) as u32).into_styled(PrimitiveStyle::with_stroke(c, 1)).draw(fb);
}
fn rrect(fb: &mut Framebuffer, x: i32, y: i32, w: u32, h: u32, c: Rgb565) {
    let _ = RoundedRectangle::with_equal_corners(rect(x, y, w, h), Size::new(6, 6)).into_styled(PrimitiveStyle::with_fill(c)).draw(fb);
}
fn dim(c: Rgb565) -> Rgb565 {
    Rgb565::new(c.r() / 3, c.g() / 3, c.b() / 3)
}

/// cos/sin of step i of n, starting at 12 o'clock (no libm: f32 methods are std)
fn pos(i: u32, n: u8, r: i32) -> (i32, i32) {
    let a = (i as f32 / n as f32) * core::f32::consts::TAU - core::f32::consts::FRAC_PI_2;
    (CX + (a.cos() * r as f32) as i32, CY + (a.sin() * r as f32) as i32)
}

pub fn draw(fb: &mut Framebuffer, st: &StatusInfo, g: &Gen, silent: bool) {
    fill(fb, 0, 0, 320, 240, SEA);
    draw_status_bar(fb, "Tidepool", st);
    let s = g.tp.step_no;
    for (li, l) in g.tp.lanes.iter().enumerate() {
        let r = RADII[li];
        ring(fb, r, RING);
        let cur = s.wrapping_sub(1) % l.n as u32;
        let fresh = g.playing && g.tp.last_hit[li].is_some_and(|h| s.wrapping_sub(h) <= 2);
        for i in 0..l.n as u32 {
            let (x, y) = pos(i, l.n, r);
            let hit = euclid_hit(l.k, l.n, l.rot, i);
            let c = LANE_COL[li];
            if g.playing && i == cur {
                dot(fb, x, y, if hit { 11 } else { 5 }, if hit && fresh { WHITE } else { c });
            } else if hit {
                dot(fb, x, y, 7, c);
            } else {
                dot(fb, x, y, 3, dim(c));
            }
        }
    }
    // centre: play state
    dot(fb, CX, CY, 34, if g.playing { rgb(20, 70, 90) } else { rgb(30, 34, 60) });
    text::draw_aligned(fb, Font::Small, if g.playing { "PLAY" } else { "STOP" }, CX, CY - 5, Align::Center, if g.playing { OK } else { DIM });

    // panel
    let (px, py) = (214, 28);
    rrect(fb, px, py, 100, 206, PANEL);
    text::draw_aligned(fb, Font::Title, "Tidepool", px + 50, py + 6, Align::Center, ACCENT);
    let rows: [(&str, String); 5] = [
        ("SCALE", scale_name(g.tp.scale).to_string()),
        ("KEY", Root { pc: g.root }.name().to_string()),
        ("TEMPO", format!("{} bpm", g.tempo)),
        ("SEED", format!("{:04}", g.seed)),
        ("LEVEL", LEVELS[g.level % LEVELS.len()].0.to_string()),
    ];
    let mut y = py + 28;
    for (k, v) in rows {
        text::draw(fb, Font::Tiny, k, px + 8, y, DIM);
        text::draw(fb, Font::BodyBold, &v, px + 8, y + 10, FG);
        y += 30;
    }
    let hint = if silent { "no audio (preview)" } else if g.playing { "Play = stop" } else { "Play = start" };
    text::draw_aligned(fb, Font::Tiny, hint, px + 50, py + 180, Align::Center, if silent { WARN } else { CYAN });
    text::draw_aligned(fb, Font::Tiny, "X = help", px + 50, py + 192, Align::Center, DIM);
}

pub fn draw_help(fb: &mut Framebuffer) {
    let (x, y, w, h) = (14, 30, 292, 200);
    rrect(fb, x, y, w as u32, h as u32, PANEL);
    text::draw(fb, Font::Title, "Tidepool", x + 12, y + 8, ACCENT);
    text::draw_aligned(fb, Font::Tiny, "any key closes", x + w - 12, y + 12, Align::Right, DIM);
    let rows: [(&str, &str); 9] = [
        ("Play", "start / stop"),
        ("Up / Dn", "tempo  40-120 bpm"),
        ("Lt / Rt", "scale"),
        ("Tri", "key up a fifth"),
        ("Cir / OK", "next seed (same = same piece)"),
        ("Sq", "level Soft / Medium / Full"),
        ("Vol", "headphone volume"),
        ("Home", "exit; music stays paused"),
        ("", "4 Euclidean rings, slow drift"),
    ];
    let mut yy = y + 32;
    for (k, a) in rows {
        text::draw(fb, Font::BodyBold, k, x + 14, yy, CYAN);
        text::draw(fb, Font::Small, a, x + 92, yy + 1, FG);
        yy += 18;
    }
}
