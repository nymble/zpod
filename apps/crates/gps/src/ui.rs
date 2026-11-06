//! GPS screen (320x240): state pill, coordinates, time, satellites, SNR bars.

use embedded_graphics::{
    pixelcolor::Rgb565,
    prelude::*,
    primitives::{PrimitiveStyle, RoundedRectangle},
};
use zpod_core::StatusInfo;
use zpod_nmea::{state::format_dd, state::format_dmm, GpsState, GpsStatus};
use zpod_ui::{fill, rect, status::draw_status_bar, text, text::Align, theme::*, widgets, Font, Framebuffer};

use crate::View;
use zpod_input::role::{ask::UsbAsk, prompt::Tone};

fn rrect(fb: &mut Framebuffer, x: i32, y: i32, w: u32, h: u32, r: u32, c: Rgb565) {
    let _ = RoundedRectangle::with_equal_corners(rect(x, y, w, h), Size::new(r, r)).into_styled(PrimitiveStyle::with_fill(c)).draw(fb);
}
fn rstroke(fb: &mut Framebuffer, x: i32, y: i32, w: u32, h: u32, r: u32, c: Rgb565) {
    let _ = RoundedRectangle::with_equal_corners(rect(x, y, w, h), Size::new(r, r)).into_styled(PrimitiveStyle::with_stroke(c, 1)).draw(fb);
}

fn pill(fb: &mut Framebuffer, x: i32, y: i32, label: &str, bg: Rgb565, fg: Rgb565) -> i32 {
    let w = text::width(Font::BodyBold, label) + 16;
    rrect(fb, x, y, w as u32, 20, 10, bg);
    text::draw(fb, Font::BodyBold, label, x + 8, y + 4, fg);
    w
}

fn state_colour(s: &GpsStatus) -> (Rgb565, Rgb565) {
    match s {
        GpsStatus::Fix3D => (OK, BG),
        GpsStatus::Fix2D => (CYAN, BG),
        GpsStatus::NoFix => (ACCENT, BG),
        GpsStatus::Stale => (rgb(200, 120, 0), BG),
        GpsStatus::Error(_) => (WARN, WHITE),
        GpsStatus::Unsupported(_) | GpsStatus::NoDevice | GpsStatus::DeviceNoData => (FAINT, WHITE),
    }
}

pub fn draw(fb: &mut Framebuffer, st: &StatusInfo, v: &View, g: &GpsState) {
    fill(fb, 0, 0, 320, 240, BG);
    draw_status_bar(fb, "GPS", st);
    let (bg, fg) = state_colour(&v.status);
    let label = match (&v.status, v.fix_age_ms) {
        (GpsStatus::Stale, Some(a)) => format!("Stale  {} s", a / 1000),
        (s, _) => s.label().to_string(),
    };
    pill(fb, 10, 26, &label, bg, fg);
    if let Some(t) = g.time {
        let ts = format!("{:02}:{:02}:{:02} UTC", t.h, t.m, t.s as u32);
        text::draw_aligned(fb, Font::BodyBold, &ts, 310, 30, Align::Right, if v.status.has_fix() { FG } else { DIM });
    }

    match &v.status {
        GpsStatus::Unsupported(why) => {
            if v.can_ask {
                message(fb, "USB host mode needed", &[why, "OK: switch USB to devices here", "(or Tools > USB; docs/USB-HOST.md)."]);
            } else {
                message(fb, "USB host mode needed", &[why, "Switch in Tools > USB, then open GPS again", "(docs/USB-HOST.md)."]);
            }
        }
        GpsStatus::NoDevice => {
            message(fb, "No GPS receiver", &["Plug in a USB GPS (e.g. GlobalSat ND-105C).", "Looking for PL2303 / ttyUSB / ttyACM ports.", "Use a powered hub if the Pi resets."]);
        }
        GpsStatus::DeviceNoData => {
            let p = v.port.clone().unwrap_or_default();
            message(fb, "No data yet", &[&p, "4800 baud, 8N1, waiting for NMEA.", "Check the receiver's power and cable."]);
        }
        GpsStatus::Error(e) => {
            let p = v.port.clone().unwrap_or_default();
            message(fb, "GPS error", &[e, &p]);
        }
        _ => {
            coords(fb, v, g);
            snr_bars(fb, g);
        }
    }
    let port = v.port.as_deref().and_then(|p| p.split_whitespace().next()).map(|n| format!("{n} 4800 8N1")).unwrap_or_default();
    if v.can_ask {
        widgets::hints(fb, &[("OK", "switch USB"), ("Home", "exit")]);
    } else if v.host {
        widgets::hints(fb, &[("OK", if v.dmm { "decimal" } else { "deg min" }), ("Sq", "computer link"), ("Home", "exit"), ("", &port)]);
    } else {
        widgets::hints(fb, &[("OK", if v.dmm { "decimal" } else { "deg min" }), ("Home", "exit"), ("", &port)]);
    }
}

/// "Switch USB?" dialog / "Switching..." / a short note, over the screen.
pub fn draw_ask(fb: &mut Framebuffer, a: &UsbAsk, note: Option<&str>) {
    if let Some(p) = a.prompt() {
        let lines: Vec<(&str, Rgb565)> = p.lines.iter().map(|(l, t)| (l.as_str(), tone(*t))).collect();
        widgets::confirm(fb, p.title, &lines, if p.ok_switches { ACCENT } else { WARN });
        widgets::hints(fb, &a.hints());
    }
    if let Some(b) = a.busy() {
        widgets::toast(fb, &b);
    } else if let Some(n) = note {
        widgets::toast(fb, n);
    }
}

fn tone(t: Tone) -> Rgb565 {
    match t {
        Tone::Normal => FG,
        Tone::Good => OK,
        Tone::Warn => WARN,
        Tone::Dim => DIM,
    }
}

fn message(fb: &mut Framebuffer, title: &str, lines: &[&str]) {
    text::draw_aligned(fb, Font::Large, title, 160, 78, Align::Center, FG);
    let mut y = 112;
    for l in lines {
        if !l.is_empty() {
            text::draw_aligned(fb, Font::Small, &text::fit(Font::Small, l, 300), 160, y, Align::Center, DIM);
            y += 18;
        }
    }
}

fn coords(fb: &mut Framebuffer, v: &View, g: &GpsState) {
    let live = v.status.has_fix();
    let c = if live { WHITE } else { FAINT };
    match (g.lat, g.lon) {
        (Some(lat), Some(lon)) => {
            let (a, b) = if v.dmm { format_dmm(lat, lon) } else { format_dd(lat, lon) };
            text::draw(fb, Font::Huge, &a, 12, 54, c);
            text::draw(fb, Font::Huge, &b, 12, 84, c);
        }
        _ => {
            text::draw(fb, Font::Huge, "--.------\u{b0}", 12, 54, FAINT);
            text::draw(fb, Font::Huge, "---.------\u{b0}", 12, 84, FAINT);
        }
    }
    let used = g.used_count().map(|n| n.to_string()).unwrap_or("-".into());
    let row = [
        ("SATS", format!("{used} / {}", g.sats_in_view())),
        ("HDOP", g.hdop.map(|h| format!("{h:.1}")).unwrap_or("-".into())),
        ("ALT", g.alt_m.filter(|_| live || v.status == GpsStatus::Stale).map(|a| format!("{a:.0} m")).unwrap_or("-".into())),
        ("DATE", g.date.map(|(d, m, y)| format!("{y}-{m:02}-{d:02}")).unwrap_or("-".into())),
    ];
    let mut x = 12;
    for (k, val) in row {
        text::draw(fb, Font::Tiny, k, x, 116, DIM);
        text::draw(fb, Font::BodyBold, &val, x, 126, FG);
        x += if k == "DATE" { 0 } else { 72 };
    }
}

/// One bar per satellite (strongest first): green = used in the fix,
/// cyan = tracked, outline = in view without signal.
fn snr_bars(fb: &mut Framebuffer, g: &GpsState) {
    let sats = g.sats();
    let (x0, y0, w, h) = (10, 148, 300, 58);
    fill(fb, x0, y0 + h, w as u32, 1, FAINT);
    if sats.is_empty() {
        text::draw_aligned(fb, Font::Small, "no satellites in view yet", 160, y0 + 24, Align::Center, DIM);
        return;
    }
    let n = sats.len().min(16) as i32;
    let pitch = (w / n).min(28);
    let bw = (pitch - 4).max(4);
    for (i, s) in sats.iter().take(16).enumerate() {
        let x = x0 + i as i32 * pitch + 2;
        match s.snr {
            Some(snr) => {
                let bh = ((snr.min(50) as i32) * h / 50).max(2);
                let c = if s.used { OK } else { CYAN };
                rrect(fb, x, y0 + h - bh, bw as u32, bh as u32, 2, c);
                if bw >= 14 {
                    text::draw_aligned(fb, Font::Tiny, &snr.to_string(), x + bw / 2, y0 + h - bh - 10, Align::Center, DIM);
                }
            }
            None => rstroke(fb, x, y0 + h - 6, bw as u32, 6, 1, FAINT),
        }
        text::draw_aligned(fb, Font::Tiny, &s.prn.to_string(), x + bw / 2, y0 + h + 3, Align::Center, if s.used { FG } else { DIM });
    }
}
