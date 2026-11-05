//! Splash, Play and Help screens for Mouse Music (zpod-ui).

use embedded_graphics::{
    pixelcolor::Rgb565,
    prelude::*,
    primitives::{Circle, Line, PrimitiveStyle, RoundedRectangle},
};
use zpod_core::StatusInfo;
use zpod_music_core::{
    format_note, Mapper, Mode, Pattern, PerfState, Voicing, AXIS_STEPS,
};
use zpod_ui::{
    fill, rect, status::draw_status_bar, text, text::Align, theme::*, widgets, Font, Framebuffer,
};

const GRID_BG: Rgb565 = rgb(12, 14, 32);
const CYAN_DIM: Rgb565 = rgb(24, 80, 110);
const ACC_DIM: Rgb565 = rgb(110, 86, 10);
const PANEL2: Rgb565 = rgb(30, 34, 66);

const GX: i32 = 4;
const GY: i32 = 24;
const GS: i32 = 196;

fn gpx(i: i32) -> i32 {
    GX + 6 + i * (GS - 12) / (AXIS_STEPS - 1)
}
fn gpy(j: i32) -> i32 {
    GY + GS - 6 - j * (GS - 12) / (AXIS_STEPS - 1)
}

fn rrect(fb: &mut Framebuffer, x: i32, y: i32, w: u32, h: u32, r: u32, c: Rgb565) {
    let _ = RoundedRectangle::with_equal_corners(rect(x, y, w, h), Size::new(r, r))
        .into_styled(PrimitiveStyle::with_fill(c))
        .draw(fb);
}
fn rstroke(fb: &mut Framebuffer, x: i32, y: i32, w: u32, h: u32, r: u32, c: Rgb565, t: u32) {
    let _ = RoundedRectangle::with_equal_corners(rect(x, y, w, h), Size::new(r, r))
        .into_styled(PrimitiveStyle::with_stroke(c, t))
        .draw(fb);
}
fn line(fb: &mut Framebuffer, x0: i32, y0: i32, x1: i32, y1: i32, c: Rgb565, t: u32) {
    let _ = Line::new(Point::new(x0, y0), Point::new(x1, y1))
        .into_styled(PrimitiveStyle::with_stroke(c, t))
        .draw(fb);
}
fn dot(fb: &mut Framebuffer, cx: i32, cy: i32, d: u32, c: Rgb565) {
    let _ = Circle::with_center(Point::new(cx, cy), d)
        .into_styled(PrimitiveStyle::with_fill(c))
        .draw(fb);
}

fn darken(fb: &mut Framebuffer, x0: usize, y0: usize, x1: usize, y1: usize, num: u32, den: u32) {
    for y in y0..y1 {
        for x in x0..x1 {
            let p = fb.get(x, y);
            let r = ((p >> 11) & 31) as u32 * num / den;
            let g = ((p >> 5) & 63) as u32 * num / den;
            let b = (p & 31) as u32 * num / den;
            fb.set_raw(x, y, ((r << 11) | (g << 5) | b) as u16);
        }
    }
}

pub fn draw_splash(fb: &mut Framebuffer, st: &StatusInfo) {
    fill(fb, 0, 0, 320, 240, BG);
    draw_status_bar(fb, "Mouse Music", st);
    text::draw_aligned(fb, Font::Title, "Mouse Music", 160, 48, Align::Center, ACCENT);
    text::draw_aligned(
        fb,
        Font::Small,
        "Inspired by Laurie Spiegel's Music Mouse",
        160,
        72,
        Align::Center,
        DIM,
    );

    let rows: [(&str, &str); 6] = [
        ("Lt / Rt", "lower voices"),
        ("Up / Dn", "upper voices"),
        ("Cir", "pattern  (hold = menu)"),
        ("Tri", "harmony mode"),
        ("X", "help"),
        ("Play", "start / stop"),
    ];
    let mut y = 100;
    for (k, a) in rows {
        let pw = text::width(Font::Tiny, k) + 6;
        rrect(fb, 70 - pw, y, pw as u32, 12, 3, FAINT);
        text::draw(fb, Font::Tiny, k, 73 - pw, y + 2, WHITE);
        text::draw(fb, Font::Small, a, 80, y + 1, FG);
        y += 16;
    }
    text::draw_aligned(fb, Font::Tiny, "any key continues", 160, 214, Align::Center, DIM);
}

pub fn draw_play(fb: &mut Framebuffer, st: &StatusInfo, perf: &PerfState, trail: &[(i32, i32)]) {
    fill(fb, 0, 0, 320, 240, BG);
    draw_status_bar(fb, "Mouse Music", st);

    rrect(fb, GX, GY, GS as u32, GS as u32, 6, GRID_BG);
    for i in 0..AXIS_STEPS {
        let c = if i % 7 == 0 { FAINT } else { rgb(30, 34, 62) };
        line(fb, gpx(i), GY + 3, gpx(i), GY + GS - 4, c, 1);
        line(fb, GX + 3, gpy(i), GX + GS - 4, gpy(i), c, 1);
    }
    for w in trail.windows(2) {
        line(fb, gpx(w[0].0), gpy(w[0].1), gpx(w[1].0), gpy(w[1].1), DIM, 1);
    }
    for &(i, j) in trail.iter().rev().skip(1).take(6) {
        dot(fb, gpx(i), gpy(j), 4, FAINT);
    }

    let (ci, cj) = (perf.x, perf.y);
    let (cx, cy) = (gpx(ci), gpy(cj));
    line(fb, cx, GY + 3, cx, GY + GS - 4, CYAN_DIM, 1);
    line(fb, GX + 3, cy, GX + GS - 4, cy, ACC_DIM, 1);
    line(fb, cx, cy + 8, cx, GY + GS - 4, CYAN, 1);
    line(fb, GX + 3, cy, cx - 8, cy, ACCENT, 1);
    let gap = perf.mapper.mode.pair_degrees();
    for i in [ci - gap, ci] {
        if i >= 0 {
            rrect(fb, gpx(i) - 3, GY + GS - 9, 7, 7, 2, CYAN);
        }
    }
    // upper markers depend on voicing
    let (uj, uj2) = match perf.mapper.voicing {
        Voicing::Contrary => {
            let cy = (AXIS_STEPS - 1) - cj;
            (cy, (cy - gap).max(0))
        }
        Voicing::Oblique => (cj, AXIS_STEPS / 2),
        Voicing::Parallel => (cj, (cj - gap).max(0)),
    };
    for j in [uj2, uj] {
        rrect(fb, GX + 3, gpy(j) - 3, 7, 7, 2, ACCENT);
    }
    dot(fb, cx, cy, 13, WHITE);
    let _ = Circle::with_center(Point::new(cx, cy), 19)
        .into_styled(PrimitiveStyle::with_stroke(FG, 1))
        .draw(fb);

    // side panel
    let px = 206;
    let mut y = 24;
    let mode_s = format_mode(perf.mapper.mode, perf.mapper.root, perf.mapper.transpose);
    let pat = if perf.pattern.is_on() {
        format_pattern(perf.pattern)
    } else {
        "Off".into()
    };
    draw_side_row(fb, px, &mut y, "MODE", &mode_s, FG);
    draw_side_row(fb, px, &mut y, "VOICING", perf.mapper.voicing.name(), FG);
    draw_side_row(fb, px, &mut y, "PATTERN", &pat, FG);
    let ts = tempo_str(perf.tempo_bpm);
    draw_side_row(fb, px, &mut y, "TEMPO", &ts, FG);
    draw_side_row(fb, px, &mut y, "AUTO", "Off", DIM);

    let vy = 172;
    fill(fb, px, vy - 4, 110, 1, FAINT);
    let mut nb = [0u8; 4];
    let n3 = format_note(perf.chord.notes[3], &mut nb).to_string();
    let mut nb = [0u8; 4];
    let n2 = format_note(perf.chord.notes[2], &mut nb).to_string();
    let mut nb = [0u8; 4];
    let n1 = format_note(perf.chord.notes[1], &mut nb).to_string();
    let mut nb = [0u8; 4];
    let n0 = format_note(perf.chord.notes[0], &mut nb).to_string();
    rrect(fb, px, vy + 2, 8, 8, 2, ACCENT);
    text::draw(fb, Font::BodyBold, &n3, px + 14, vy, FG);
    text::draw(fb, Font::BodyBold, &n2, px + 42, vy, FG);
    text::draw(fb, Font::Tiny, "upper", px + 72, vy + 2, DIM);
    rrect(fb, px, vy + 22, 8, 8, 2, CYAN);
    text::draw(fb, Font::BodyBold, &n1, px + 14, vy + 20, FG);
    text::draw(fb, Font::BodyBold, &n0, px + 42, vy + 20, FG);
    text::draw(fb, Font::Tiny, "lower", px + 72, vy + 22, DIM);

    for b in 0..4 {
        let c = if b == (perf.arp_step as i32) { ACCENT } else { FAINT };
        dot(fb, px + 76 + b * 9, 156, 6, c);
    }

    widgets::hints(
        fb,
        &[
            ("Cir", "pattern"),
            ("Tri", "mode"),
            ("Sq", "tempo"),
            ("X", "help"),
            ("Play", "start/stop"),
        ],
    );
}

fn draw_side_row(fb: &mut Framebuffer, px: i32, y: &mut i32, label: &str, value: &str, c: Rgb565) {
    text::draw(fb, Font::Tiny, label, px, *y, DIM);
    text::draw(fb, Font::BodyBold, value, px, *y + 10, c);
    *y += 29;
}

fn format_mode(mode: Mode, root: zpod_music_core::Root, transpose: i32) -> String {
    format!("{}  {}", mode.short(), root.transpose(transpose).name())
}
fn format_pattern(p: Pattern) -> String {
    format!("{}  1/16", p.name())
}
fn tempo_str(bpm: u32) -> String {
    format!("{bpm} BPM")
}

pub fn draw_help(fb: &mut Framebuffer, st: &StatusInfo, perf: &PerfState, trail: &[(i32, i32)]) {
    draw_play(fb, st, perf, trail);
    darken(fb, 0, 20, 320, 240, 1, 3);
    let (x, y, w, h) = (10, 26, 300, 208);
    rrect(fb, x, y, w, h, 10, PANEL);
    rstroke(fb, x, y, w, h, 10, ACCENT, 2);
    text::draw(fb, Font::Title, "Buttons", x + 12, y + 8, ACCENT);
    text::draw_aligned(fb, Font::Tiny, "any key closes", x + w as i32 - 12, y + 12, Align::Right, DIM);
    let rows: [(&str, &str, &str); 11] = [
        ("Lt / Rt", "lower voices", "hold = glide"),
        ("Up / Dn", "upper voices", "hold = glide"),
        ("Cir", "pattern on / off", "hold = menu (soon)"),
        ("Tri", "next harmony mode", ""),
        ("Tri + Pad", "transpose", "Up/Dn;  voicing Lt/Rt"),
        ("Sq", "tempo +/− 4", "hold + Up/Dn"),
        ("Sq + Lt/Rt", "density (soon)", ""),
        ("X", "this help", ""),
        ("Play", "start / stop all sound", ""),
        ("Home", "exit; music stays paused", ""),
        ("Vol", "volume", ""),
    ];
    let mut yy = y + 28;
    for (k, a, b) in rows {
        let pw = text::width(Font::Tiny, k) + 6;
        rrect(fb, x + 70 - pw, yy + 1, pw as u32, 12, 3, FAINT);
        text::draw(fb, Font::Tiny, k, x + 73 - pw, yy + 3, WHITE);
        let aw = text::draw(fb, Font::Small, a, x + 78, yy + 1, FG);
        if !b.is_empty() {
            text::draw(fb, Font::Tiny, b, x + 78 + aw + 6, yy + 3, DIM);
        }
        yy += 15;
    }
    let _ = (PANEL2, Mapper::default());
}

pub fn draw_toast(fb: &mut Framebuffer, msg: &str) {
    let w = text::width(Font::Body, msg) + 24;
    let x = (320 - w as i32) / 2;
    rrect(fb, x, 200, w as u32, 22, 6, PANEL2);
    text::draw_aligned(fb, Font::Body, msg, 160, 205, Align::Center, FG);
}
