//! Phase-1 screenshots for Mouse Music (splash + play).
use embedded_graphics::{
    pixelcolor::Rgb565,
    prelude::*,
    primitives::{Circle, Line, PrimitiveStyle, RoundedRectangle},
};
use zpod_core::{time::DateTime, PlayState, StatusInfo, WifiMode};
use zpod_ui::{
    fill, rect, status::draw_status_bar, text, text::Align, theme::*, widgets, Font, Framebuffer,
};

const GRID_BG: Rgb565 = rgb(12, 14, 32);
const CYAN_DIM: Rgb565 = rgb(24, 80, 110);
const ACC_DIM: Rgb565 = rgb(110, 86, 10);

fn st() -> StatusInfo {
    StatusInfo {
        time: Some(DateTime { year: 2026, month: 11, day: 5, hour: 10, minute: 8, second: 0, weekday: 4 }),
        battery_pct: Some(87.0),
        charging: false,
        wifi: true,
        wifi_mode: WifiMode::Client { bars: 3, connected: true },
        usb: true,
        volume_pct: Some(46),
        play: PlayState::Paused,
    }
}

fn save(fb: &Framebuffer, path: &str) {
    let f = std::fs::File::create(path).unwrap();
    let mut e = png::Encoder::new(std::io::BufWriter::new(f), 320, 240);
    e.set_color(png::ColorType::Rgb);
    e.set_depth(png::BitDepth::Eight);
    e.write_header().unwrap().write_image_data(&fb.to_rgb888()).unwrap();
    println!("wrote {path}");
}

fn rrect(fb: &mut Framebuffer, x: i32, y: i32, w: u32, h: u32, r: u32, c: Rgb565) {
    let _ = RoundedRectangle::with_equal_corners(rect(x, y, w, h), Size::new(r, r))
        .into_styled(PrimitiveStyle::with_fill(c))
        .draw(fb);
}
fn line(fb: &mut Framebuffer, x0: i32, y0: i32, x1: i32, y1: i32, c: Rgb565, t: u32) {
    let _ = Line::new(Point::new(x0, y0), Point::new(x1, y1)).into_styled(PrimitiveStyle::with_stroke(c, t)).draw(fb);
}
fn dot(fb: &mut Framebuffer, cx: i32, cy: i32, d: u32, c: Rgb565) {
    let _ = Circle::with_center(Point::new(cx, cy), d).into_styled(PrimitiveStyle::with_fill(c)).draw(fb);
}

const GX: i32 = 4;
const GY: i32 = 24;
const GS: i32 = 196;
const STEPS: i32 = 15;
fn gpx(i: i32) -> i32 { GX + 6 + i * (GS - 12) / (STEPS - 1) }
fn gpy(j: i32) -> i32 { GY + GS - 6 - j * (GS - 12) / (STEPS - 1) }

fn splash() -> Framebuffer {
    let mut fb = Framebuffer::default();
    fill(&mut fb, 0, 0, 320, 240, BG);
    draw_status_bar(&mut fb, "Mouse", &st());
    text::draw_aligned(&mut fb, Font::Title, "Mouse Music", 160, 48, Align::Center, ACCENT);
    text::draw_aligned(&mut fb, Font::Small, "Inspired by Laurie Spiegel's Music Mouse", 160, 72, Align::Center, DIM);
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
        rrect(&mut fb, 70 - pw, y, pw as u32, 12, 3, FAINT);
        text::draw(&mut fb, Font::Tiny, k, 73 - pw, y + 2, WHITE);
        text::draw(&mut fb, Font::Small, a, 80, y + 1, FG);
        y += 16;
    }
    text::draw_aligned(&mut fb, Font::Tiny, "any key continues", 160, 214, Align::Center, DIM);
    fb
}

fn play() -> Framebuffer {
    let mut fb = Framebuffer::default();
    fill(&mut fb, 0, 0, 320, 240, BG);
    draw_status_bar(&mut fb, "Mouse", &st());
    rrect(&mut fb, GX, GY, GS as u32, GS as u32, 6, GRID_BG);
    for i in 0..STEPS {
        let c = if i % 7 == 0 { FAINT } else { rgb(30, 34, 62) };
        line(&mut fb, gpx(i), GY + 3, gpx(i), GY + GS - 4, c, 1);
        line(&mut fb, GX + 3, gpy(i), GX + GS - 4, gpy(i), c, 1);
    }
    let trail = [(3, 4), (4, 4), (5, 6), (6, 7), (7, 7), (8, 9), (9, 10)];
    for w in trail.windows(2) {
        line(&mut fb, gpx(w[0].0), gpy(w[0].1), gpx(w[1].0), gpy(w[1].1), DIM, 1);
    }
    let (ci, cj) = (9, 10);
    let (cx, cy) = (gpx(ci), gpy(cj));
    line(&mut fb, cx, GY + 3, cx, GY + GS - 4, CYAN_DIM, 1);
    line(&mut fb, GX + 3, cy, GX + GS - 4, cy, ACC_DIM, 1);
    line(&mut fb, cx, cy + 8, cx, GY + GS - 4, CYAN, 1);
    line(&mut fb, GX + 3, cy, cx - 8, cy, ACCENT, 1);
    for i in [ci - 2, ci] { rrect(&mut fb, gpx(i) - 3, GY + GS - 9, 7, 7, 2, CYAN); }
    for j in [cj - 2, cj] { rrect(&mut fb, GX + 3, gpy(j) - 3, 7, 7, 2, ACCENT); }
    dot(&mut fb, cx, cy, 13, WHITE);
    let _ = Circle::with_center(Point::new(cx, cy), 19).into_styled(PrimitiveStyle::with_stroke(FG, 1)).draw(&mut fb);
    let px = 206;
    let mut y = 24;
    for (l, v, c) in [
        ("MODE", "Diatonic  C", FG),
        ("VOICING", "Contrary", FG),
        ("PATTERN", "Arp up  1/16", FG),
        ("TEMPO", "112 BPM", FG),
        ("AUTO", "Off", DIM),
    ] {
        text::draw(&mut fb, Font::Tiny, l, px, y, DIM);
        text::draw(&mut fb, Font::BodyBold, v, px, y + 10, c);
        y += 29;
    }
    let vy = 172;
    fill(&mut fb, px, vy - 4, 110, 1, FAINT);
    rrect(&mut fb, px, vy + 2, 8, 8, 2, ACCENT);
    text::draw(&mut fb, Font::BodyBold, "F5", px + 14, vy, FG);
    text::draw(&mut fb, Font::BodyBold, "D5", px + 42, vy, FG);
    text::draw(&mut fb, Font::Tiny, "upper", px + 72, vy + 2, DIM);
    rrect(&mut fb, px, vy + 22, 8, 8, 2, CYAN);
    text::draw(&mut fb, Font::BodyBold, "E3", px + 14, vy + 20, FG);
    text::draw(&mut fb, Font::BodyBold, "C3", px + 42, vy + 20, FG);
    text::draw(&mut fb, Font::Tiny, "lower", px + 72, vy + 22, DIM);
    for b in 0..4 {
        let c = if b == 1 { ACCENT } else { FAINT };
        dot(&mut fb, px + 76 + b * 9, 156, 6, c);
    }
    widgets::hints(&mut fb, &[("Cir", "pattern"), ("Tri", "mode"), ("Sq", "tempo"), ("X", "help"), ("Play", "start/stop")]);
    fb
}

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "/tmp/mm-phase1".into());
    std::fs::create_dir_all(&out).unwrap();
    save(&splash(), &format!("{out}/splash.png"));
    save(&play(), &format!("{out}/play.png"));
}
