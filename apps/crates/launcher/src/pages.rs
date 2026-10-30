//! M6 screens: Battery, Signal meter, Channels, Hotspot, Drop box / Book
//! catalog, first-boot notice. Pure drawing; the data comes from the caller.

use embedded_graphics::{pixelcolor::Rgb565, prelude::*, primitives::*};
use zpod_hal::battery::{self, Sample, Session, State};
use zpod_hal::wifi::{self, Channel, Hotspot};
use zpod_ui::{
    fill, rect,
    text::{self, Align},
    theme::*,
    widgets, Font, Framebuffer,
};

use crate::{server, wifitools};

const Y0: i32 = STATUS_H;
const H: i32 = 240 - STATUS_H - widgets::HINTS_H;

fn clear(fb: &mut Framebuffer) {
    fill(fb, 0, Y0, 320, H as u32, BG);
}

fn line(fb: &mut Framebuffer, a: (i32, i32), b: (i32, i32), c: Rgb565, w: u32) {
    let _ = Line::new(Point::new(a.0, a.1), Point::new(b.0, b.1)).into_styled(PrimitiveStyle::with_stroke(c, w)).draw(fb);
}

fn soc_colour(p: f32) -> Rgb565 {
    if p < 15.0 {
        WARN
    } else if p < 30.0 {
        ACCENT
    } else {
        OK
    }
}

pub struct BattView {
    /// (SOC %, volts, CRATE %/h)
    pub now: Option<(f32, f32, f32)>,
    pub samples: Vec<Sample>,
    pub sessions: Vec<Session>,
    pub unix_now: i64,
    pub low_pct: f32,
}

/// Battery: big charge figure, state, time left, last full charge, typical
/// runtime, a %-vs-time chart since the last charge. Tight, few labels.
pub fn draw_battery(fb: &mut Framebuffer, v: &BattView) {
    clear(fb);
    let Some((soc, volts, rate)) = v.now else {
        text::draw_aligned(fb, Font::Digits58, "--", 160, Y0 + 24, Align::Center, FAINT);
        text::draw_aligned(fb, Font::Title, "No fuel gauge", 160, Y0 + 104, Align::Center, DIM);
        text::draw_aligned(fb, Font::Small, "MAX17048 (I2C 0x36) is not answering.", 160, Y0 + 128, Align::Center, FAINT);
        text::draw_aligned(fb, Font::Small, "History, if any, is in /data/zpod/battery.log", 160, Y0 + 144, Align::Center, FAINT);
        widgets::hints(fb, &[("Lt/X", "back"), ("Home", "top")]);
        return;
    };
    let st = battery::state(soc, rate);
    let col = if st == State::Discharging { soc_colour(soc) } else { OK };
    // big figure
    let pct = format!("{:.0}", soc.round());
    let w = text::width(Font::Digits58, &pct);
    text::draw(fb, Font::Digits58, &pct, 14, Y0 + 8, FG);
    text::draw(fb, Font::Huge, "%", 14 + w + 3, Y0 + 12, DIM);
    // state + time left (right column)
    let (state, sc) = match st {
        State::Charging => ("Charging", OK),
        State::Full => ("Charged", OK),
        State::Discharging => ("On battery", FG),
    };
    let x = 176;
    text::draw(fb, Font::Title, state, x, Y0 + 10, sc);
    let left = battery::time_left(&v.samples, v.unix_now, soc, rate);
    let (lt, sub) = match (st, left) {
        (State::Full, _) => ("Full".to_string(), "on the charger"),
        (State::Charging, Some(s)) => (battery::human_duration(s), "until full"),
        (State::Charging, None) => ("--".to_string(), "until full"),
        (State::Discharging, Some(s)) => (battery::human_duration(s), "left (estimate)"),
        (State::Discharging, None) => ("--".to_string(), "left: learning..."),
    };
    text::draw_fit(fb, Font::Large, &lt, x, Y0 + 32, 138, FG);
    text::draw(fb, Font::Small, sub, x, Y0 + 56, DIM);
    // level bar
    let (bx, by, bw) = (14, Y0 + 78, 292u32);
    let _ = RoundedRectangle::with_equal_corners(rect(bx, by, bw, 6), Size::new(3, 3)).into_styled(PrimitiveStyle::with_fill(PANEL)).draw(fb);
    let fw = ((soc.clamp(0.0, 100.0) / 100.0) * bw as f32) as u32;
    if fw > 3 {
        let _ = RoundedRectangle::with_equal_corners(rect(bx, by, fw, 6), Size::new(3, 3)).into_styled(PrimitiveStyle::with_fill(col)).draw(fb);
    }
    // facts: two columns
    let last = battery::last_charge(&v.samples);
    let full = match last {
        Some((t, p)) if st == State::Discharging => format!("{} ago{}", battery::human_duration(v.unix_now - t), if p < 97.0 { format!(" ({p:.0}%)") } else { String::new() }),
        Some(_) => "now".into(),
        None => "not seen yet".into(),
    };
    let typ = battery::typical_runtime(&v.sessions, battery::current_session(&v.samples).filter(|_| st == State::Discharging))
        .map(|(s, n)| format!("{}{}", battery::human_duration(s), if n > 1 { format!("  ({n})") } else { String::new() }))
        .unwrap_or_else(|| "needs a longer run".into());
    let facts = [("Last charged", full), ("Full charge lasts", typ), ("Cell", format!("{volts:.2} V   {rate:+.1} %/h"))];
    for (i, (k, val)) in facts.iter().enumerate() {
        let y = Y0 + 90 + i as i32 * 15;
        text::draw(fb, Font::Small, k, 14, y, DIM);
        text::draw_fit(fb, Font::Small, val, 124, y, 186, FG);
    }
    // chart: % since the last charge
    let (cx, cy, cw, ch) = (34, Y0 + 138, 274, 52);
    for (p, lbl) in [(100, "100"), (50, "50"), (0, "0")] {
        let y = cy + ch - p * ch / 100;
        line(fb, (cx, y), (cx + cw, y), if p == 0 { FAINT } else { PANEL }, 1);
        text::draw_aligned(fb, Font::Tiny, lbl, cx - 4, y - 4, Align::Right, FAINT);
    }
    if v.low_pct > 0.0 {
        let y = cy + ch - (v.low_pct as i32 * ch / 100);
        line(fb, (cx, y), (cx + cw, y), rgb(90, 30, 30), 1);
    }
    let curve = battery::discharge_curve(&v.samples, cw as usize / 2);
    if curve.len() >= 2 {
        let (t0, t1) = (curve[0].0, curve[curve.len() - 1].0.max(curve[0].0 + 60));
        let px = |t: i64| cx + ((t - t0) as f32 / (t1 - t0) as f32 * cw as f32) as i32;
        let py = |p: f32| cy + ch - (p.clamp(0.0, 100.0) / 100.0 * ch as f32) as i32;
        // soft fill under the curve, then the line
        for w in curve.windows(2) {
            let (xa, xb) = (px(w[0].0), px(w[1].0));
            for xx in xa..=xb.min(cx + cw) {
                let f = if xb > xa { (xx - xa) as f32 / (xb - xa) as f32 } else { 0.0 };
                let yy = py(w[0].1 + (w[1].1 - w[0].1) * f);
                line(fb, (xx, yy + 2), (xx, cy + ch - 1), rgb(14, 40, 34), 1);
            }
        }
        for w in curve.windows(2) {
            line(fb, (px(w[0].0), py(w[0].1)), (px(w[1].0), py(w[1].1)), col, 2);
        }
        let span = battery::human_duration(t1 - t0);
        text::draw(fb, Font::Tiny, &format!("since unplugged, {span}"), cx, cy + ch + 3, FAINT);
        text::draw_aligned(fb, Font::Tiny, "now", cx + cw, cy + ch + 3, Align::Right, FAINT);
    } else {
        let msg = if st == State::Discharging { "Chart appears after a few minutes on battery" } else { "Chart starts when you unplug" };
        text::draw_aligned(fb, Font::Small, msg, cx + cw / 2, cy + 20, Align::Center, FAINT);
    }
    widgets::hints(fb, &[("Lt/X", "back"), ("Home", "top"), ("", "logged every minute")]);
}

/// Bars 0-4 drawn like a phone's.
fn bars(fb: &mut Framebuffer, x: i32, y: i32, n: u8, c: Rgb565) {
    for i in 0..4 {
        let h = 6 + i * 6;
        fill(fb, x + i * 9, y + 24 - h, 6, h as u32, if (i as u8) < n { c } else { PANEL });
    }
}

fn dbm_colour(d: i32) -> Rgb565 {
    match crate::tools::bars(d as f32) {
        4 | 3 => OK,
        2 => ACCENT,
        1 => WARN,
        _ => FAINT,
    }
}

pub fn draw_meter(fb: &mut Framebuffer, w: &wifitools::WifiTools) {
    clear(fb);
    let (name, sub) = match (&w.target, &w.link) {
        (Some(t), _) => (if t.ssid.is_empty() { "(hidden)".to_string() } else { t.ssid.clone() }, format!("ch {}  {}  (scan every ~5 s)", t.channel, t.bssid)),
        (None, Some(l)) => {
            let ch = zpod_hal::scan::freq_to_channel(l.freq_mhz);
            (l.ssid.clone(), format!("connected  ch {ch}  {}{}", l.bssid, l.tx_mbit.map(|r| format!("  {r:.0} Mbit/s")).unwrap_or_default()))
        }
        (None, None) => ("Not connected".to_string(), "Pick a network: Wi-Fi scanner > OK > OK".to_string()),
    };
    text::draw_fit(fb, Font::Title, &name, 14, Y0 + 6, 292, FG);
    text::draw_fit(fb, Font::Small, &sub, 14, Y0 + 26, 292, DIM);
    match w.last {
        Some(d) => {
            let c = dbm_colour(d);
            let s = d.to_string();
            let tw = text::width(Font::Digits58, &s);
            text::draw(fb, Font::Digits58, &s, 14, Y0 + 46, c);
            text::draw(fb, Font::Large, "dBm", 20 + tw, Y0 + 84, DIM);
            bars(fb, 210, Y0 + 52, crate::tools::bars(d as f32), c);
            text::draw(fb, Font::Large, &format!("{}%", wifi::quality(d)), 210, Y0 + 84, FG);
            let label = match crate::tools::bars(d as f32) {
                4 => "excellent",
                3 => "good",
                2 => "fair",
                1 => "weak",
                _ => "very weak",
            };
            text::draw(fb, Font::Small, label, 258, Y0 + 62, DIM);
        }
        None if w.meter_scan.busy.is_some() || (w.target.is_none() && w.link.is_none() && w.meter_at.is_none()) => {
            text::draw(fb, Font::Large, "Measuring...", 14, Y0 + 64, DIM);
        }
        None => {
            text::draw(fb, Font::Large, if w.target.is_some() { "Not heard in the last scan" } else { "No signal" }, 14, Y0 + 64, WARN);
        }
    }
    // history: last 90 readings, -90..-30 dBm
    let (cx, cy, cw, ch) = (34, Y0 + 116, 274, 70);
    for (d, lbl) in [(-30, "-30"), (-60, "-60"), (-90, "-90")] {
        let y = cy + (-30 - d) * ch / 60;
        line(fb, (cx, y), (cx + cw, y), if d == -90 { FAINT } else { PANEL }, 1);
        text::draw_aligned(fb, Font::Tiny, lbl, cx - 4, y - 4, Align::Right, FAINT);
    }
    let pts: Vec<i32> = w.history.iter().copied().collect();
    let step = cw as f32 / (wifitools::METER_POINTS - 1) as f32;
    let x0 = cx + cw - ((pts.len().max(1) - 1) as f32 * step) as i32;
    let py = |d: i32| cy + ((-30 - d.clamp(-90, -30)) * ch / 60);
    for (i, pair) in pts.windows(2).enumerate() {
        let xa = x0 + (i as f32 * step) as i32;
        let xb = x0 + ((i + 1) as f32 * step) as i32;
        line(fb, (xa, py(pair[0])), (xb, py(pair[1])), dbm_colour(pair[1]), 2);
    }
    if pts.len() == 1 {
        fill(fb, cx + cw - 2, py(pts[0]) - 1, 3, 3, dbm_colour(pts[0]));
    }
    let span = if w.target.is_some() { "last ~7 min" } else { "last 90 s" };
    text::draw(fb, Font::Tiny, span, cx, cy + ch + 3, FAINT);
    widgets::hints(fb, &[("Lt/X", "back"), ("Home", "top"), ("", "walk around: higher = better")]);
}

pub fn draw_channels(fb: &mut Framebuffer, chans: &[Channel], connected: Option<u32>, scanning: bool, age: &str, total: usize) {
    clear(fb);
    let best = wifi::best_channel(chans);
    let max = chans.iter().map(|c| c.aps).max().unwrap_or(0).max(3);
    let (x0, base, hmax) = (22, Y0 + 150, 112);
    let bw = 20;
    for c in chans {
        let x = x0 + (c.ch as i32 - 1) * 22;
        // overlap load as a faint backdrop, APs on the channel as the bar
        let lh = ((c.load / max as f32) * hmax as f32).min(hmax as f32) as i32;
        if lh > 0 {
            fill(fb, x + 1, base - lh, (bw - 2) as u32, lh as u32, rgb(24, 28, 54));
        }
        let h = (c.aps as i32 * hmax / max as i32).min(hmax);
        let col = if Some(c.ch) == connected {
            CYAN
        } else if c.aps == 0 {
            FAINT
        } else if c.aps >= 3 {
            WARN
        } else {
            ACCENT
        };
        if h > 0 {
            fill(fb, x + 3, base - h, (bw - 6) as u32, h as u32, col);
            text::draw_aligned(fb, Font::Tiny, &c.aps.to_string(), x + bw / 2, base - h - 11, Align::Center, FG);
        }
        let lc = if c.ch == best { OK } else if [1, 6, 11].contains(&c.ch) { FG } else { DIM };
        text::draw_aligned(fb, Font::Small, &c.ch.to_string(), x + bw / 2, base + 4, Align::Center, lc);
    }
    line(fb, (x0 - 2, base), (x0 + 13 * 22, base), FAINT, 1);
    let n24: u32 = chans.iter().map(|c| c.aps).sum();
    if scanning && total == 0 {
        text::draw(fb, Font::Body, "Scanning...", 12, Y0 + 6, DIM);
    } else {
        text::draw(fb, Font::Body, &format!("{n24} networks"), 12, Y0 + 6, FG);
        text::draw_aligned(fb, Font::BodyBold, &format!("least busy: ch {best}"), 308, Y0 + 6, Align::Right, OK);
    }
    text::draw_fit(fb, Font::Small, "bar = networks on it, shade = overlap", 12, Y0 + 172, 296, FAINT);
    let tail = match connected {
        Some(c) => format!("cyan = yours (ch {c}). 2.4 GHz only."),
        None => "The Zero W radio is 2.4 GHz only.".to_string(),
    };
    text::draw_fit(fb, Font::Small, &tail, 12, Y0 + 186, 296, DIM);
    widgets::hints(fb, &[("Sq", if scanning { "scanning" } else { "rescan" }), ("Lt/X", "back"), ("", age)]);
}

pub fn draw_hotspot(fb: &mut Framebuffer, h: Option<&Hotspot>, busy: Option<&str>, home: Option<&str>, srv: &server::State) {
    clear(fb);
    let active = h.is_some_and(|h| h.active);
    if let Some(b) = busy {
        text::draw_aligned(fb, Font::Huge, b, 160, Y0 + 70, Align::Center, ACCENT);
        text::draw_aligned(fb, Font::Small, "This can take 10-20 s.", 160, Y0 + 104, Align::Center, DIM);
        widgets::hints(fb, &[("", "working...")]);
        return;
    }
    if active {
        let h = h.unwrap();
        text::draw(fb, Font::Small, "Network", 14, Y0 + 8, DIM);
        text::draw_fit(fb, Font::Huge, &h.ssid, 14, Y0 + 20, 292, FG);
        text::draw(fb, Font::Small, "Password", 14, Y0 + 52, DIM);
        text::draw_fit(fb, Font::Large, &h.psk, 14, Y0 + 66, 292, ACCENT);
        let devs = format!("{} device{}  \u{b7}  {}  \u{b7}  ch {}", h.clients, if h.clients == 1 { "" } else { "s" }, wifi::HOTSPOT_IP, h.channel);
        text::draw_fit(fb, Font::Body, &devs, 14, Y0 + 94, 292, FG);
        let web = match (srv.dropbox, srv.opds) {
            (false, false) => "Drop box, books: off (Tools)".to_string(),
            (true, true) => format!("http://{}:8080/  + /opds", wifi::HOTSPOT_IP),
            (true, false) => format!("http://{}:8080/", wifi::HOTSPOT_IP),
            (false, true) => format!("http://{}:8080/opds", wifi::HOTSPOT_IP),
        };
        text::draw_fit(fb, Font::Body, &web, 14, Y0 + 114, 292, if srv.dropbox || srv.opds { CYAN } else { DIM });
        fill(fb, 10, Y0 + 140, 300, 46, PANEL);
        text::draw_fit(fb, Font::BodyBold, "Off your home Wi-Fi while on", 18, Y0 + 145, 286, WARN);
        text::draw_fit(fb, Font::Small, "Zero W can't do both. USB still works.", 18, Y0 + 166, 286, DIM);
        widgets::hints(fb, &[("OK", "turn off"), ("Tri", "new password"), ("Lt/X", "back")]);
    } else {
        text::draw(fb, Font::Huge, "Hotspot off", 14, Y0 + 8, FG);
        let name = h.map(|h| h.ssid.clone()).unwrap_or_else(|| wifi::hotspot_ssid(zpod_hal::sys::mac_address("wlan0").as_deref()));
        text::draw(fb, Font::Body, "OK = ZPOD becomes the network", 14, Y0 + 42, FG);
        text::draw_fit(fb, Font::BodyBold, &name, 14, Y0 + 60, 292, CYAN);
        text::draw_fit(fb, Font::Small, "WPA2. Password made here, shown here.", 14, Y0 + 80, 292, DIM);
        text::draw_fit(fb, Font::Small, "For the drop box and books away from home.", 14, Y0 + 94, 292, DIM);
        fill(fb, 10, Y0 + 116, 300, 70, PANEL);
        let off = match home {
            Some(s) if !s.is_empty() => format!("Leaves \"{s}\""),
            _ => "Leaves your home Wi-Fi".to_string(),
        };
        text::draw_fit(fb, Font::BodyBold, &off, 18, Y0 + 122, 286, WARN);
        text::draw_fit(fb, Font::Small, "The Zero W radio can't be on Wi-Fi", 18, Y0 + 142, 286, DIM);
        text::draw_fit(fb, Font::Small, "and a hotspot at once. OK again rejoins.", 18, Y0 + 156, 286, DIM);
        text::draw_fit(fb, Font::Small, "USB keeps working.", 18, Y0 + 170, 286, DIM);
        widgets::hints(fb, &[("OK", "turn on"), ("Lt/X", "back"), ("Home", "top")]);
    }
}

pub fn draw_server(fb: &mut Framebuffer, opds: bool, s: &server::State, hotspot_on: bool) {
    clear(fb);
    let on = if opds { s.opds } else { s.dropbox };
    let what = if opds { "Book catalog" } else { "Drop box" };
    text::draw(fb, Font::Huge, &format!("{what} {}", if on { "on" } else { "off" }), 14, Y0 + 8, if on { OK } else { FG });
    let mut y = Y0 + 42;
    if on {
        for (k, url, note) in server::urls(opds, hotspot_on) {
            text::draw(fb, Font::Small, k, 14, y + 3, DIM);
            text::draw_fit(fb, Font::BodyBold, &url, 76, y, 232, CYAN);
            if !note.is_empty() {
                text::draw(fb, Font::Tiny, if s.token { note } else { "home Wi-Fi: set a token first" }, 76, y + 16, if s.token { FAINT } else { WARN });
                y += 10;
            }
            y += 22;
        }
        if !s.running {
            text::draw_fit(fb, Font::Small, "Server not answering yet; it starts on the first visit.", 14, y, 292, FAINT);
        }
        y += 8;
    } else {
        y += 4;
    }
    let lines: &[&str] = if opds {
        &["OPDS feed of /data/books for e-readers.", "KOReader: Search > OPDS catalog > +", "Thorium, Marvin: add an OPDS feed."]
    } else {
        &["Browse, download music and books;", "upload files into /data/inbox.", "Open it in a browser (Mac or phone)."]
    };
    for l in lines {
        text::draw_fit(fb, Font::Small, l, 14, y, 292, DIM);
        y += 14;
    }
    let info = if opds { format!("{} books in /data/books", s.books) } else { format!("{} in inbox, max {} MB/file", s.inbox, s.max_mb) };
    text::draw_fit(fb, Font::Small, &info, 14, y + 4, 292, FG);
    let wifi_note = if s.token { "Home Wi-Fi: token from drop.conf" } else { "USB + ZPOD hotspot only" };
    text::draw_fit(fb, Font::Small, wifi_note, 14, y + 20, 292, FAINT);
    widgets::hints(fb, &[("OK", if on { "turn off" } else { "turn on" }), ("Lt/X", "back"), ("", "off by default")]);
}

pub fn draw_notice(fb: &mut Framebuffer, lines: &[String]) {
    clear(fb);
    fill(fb, 8, Y0 + 6, 304, 30, rgb(60, 36, 0));
    text::draw(fb, Font::Title, "Setup files used at this boot", 16, Y0 + 13, ACCENT);
    let mut y = Y0 + 44;
    'outer: for l in lines {
        fill(fb, 14, y + 5, 4, 4, ACCENT);
        for (i, part) in wrap(Font::Body, l, 284).into_iter().enumerate() {
            if y > Y0 + 146 {
                break 'outer;
            }
            text::draw_fit(fb, Font::Body, &part, if i == 0 { 24 } else { 34 }, y, 284, FG);
            y += 17;
        }
        y += 3;
    }
    text::draw_fit(fb, Font::Small, "Added to bootfs after the first boot.", 14, Y0 + 170, 292, DIM);
    text::draw_fit(fb, Font::Small, "Log: journalctl -u zpod-firstuser", 14, Y0 + 185, 292, FAINT);
    widgets::hints(fb, &[("OK", "got it"), ("Home", "menu")]);
}

/// Greedy word wrap to `max_w` pixels (a word longer than a line is cut by draw_fit).
pub fn wrap(font: Font, s: &str, max_w: i32) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    for w in s.split_whitespace() {
        let cand = if cur.is_empty() { w.to_string() } else { format!("{cur} {w}") };
        if !cur.is_empty() && text::width(font, &cand) > max_w {
            out.push(std::mem::take(&mut cur));
            cur = w.to_string();
        } else {
            cur = cand;
        }
    }
    if !cur.is_empty() || out.is_empty() {
        out.push(cur);
    }
    out
}
