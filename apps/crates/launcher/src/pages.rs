//! M6 screens: Battery, Signal meter, Channels, Hotspot, Drop box / Book
//! catalog, first-boot notice; test3: Wi-Fi status card, mode chooser,
//! hotspot devices, channel utilization curves, devices & addresses, USB.
//! Pure drawing; the data comes from the caller.

use embedded_graphics::{pixelcolor::Rgb565, prelude::*, primitives::*};
use zpod_hal::battery::{self, Sample, Session, State};
use zpod_hal::scan::WifiNet;
use zpod_hal::wifi::{self, Hotspot, Mode, Peer};
use zpod_ui::{
    fill, rect,
    text::{self, Align},
    theme::*,
    widgets, Font, Framebuffer,
};

use crate::{menu::WifiRow, server, usb, wifitools};

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
    let d = &w.detail;
    let (name, sub) = match (&w.target, &w.link) {
        (Some(t), _) if t.bssid.is_empty() => (t.ssid.clone(), "strongest access point  (scan every ~5 s)".to_string()),
        (Some(t), _) => (if t.ssid.is_empty() { "(hidden)".to_string() } else { t.ssid.clone() }, format!("ch {}  {}  (scan every ~5 s)", t.channel, t.bssid)),
        (None, Some(l)) => {
            let ch = zpod_hal::scan::freq_to_channel(l.freq_mhz);
            (l.ssid.clone(), format!("connected  ch {ch}  {}{}", l.bssid, l.tx_mbit.map(|r| format!("  {r:.0} Mbit/s")).unwrap_or_default()))
        }
        (None, None) => ("Not connected".to_string(), "Pick a network: Wi-Fi > Networks nearby".to_string()),
    };
    let sub = if d.security.is_empty() && d.saved_uuid.is_none() {
        sub
    } else {
        let mut x = sub;
        if !d.security.is_empty() {
            x = format!("{}  {x}", d.security);
        }
        if d.saved_uuid.is_some() && !d.connected {
            x = format!("saved  {x}");
        }
        x
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
    let ok = if d.connected || (w.target.is_none() && w.link.is_some()) {
        "connected"
    } else if d.saved_uuid.is_some() || d.security == "Open" {
        "join"
    } else {
        "join (needs zpod-wifi.txt)"
    };
    widgets::hints(fb, &[("OK", ok), ("Lt/X", "back"), ("", "higher = better")]);
}

pub fn draw_hotspot(fb: &mut Framebuffer, h: Option<&Hotspot>, busy: Option<&str>, home: Option<&str>, srv: &server::State, reveal: bool) {
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
        if reveal {
            text::draw_fit(fb, Font::Large, &h.psk, 14, Y0 + 66, 292, ACCENT);
        } else {
            text::draw(fb, Font::Large, "****-****-****-****", 14, Y0 + 66, DIM);
            text::draw(fb, Font::Small, "OK = show", 230, Y0 + 72, FAINT);
        }
        let devs = format!("{} device{}  \u{b7}  {}  \u{b7}  ch {}", h.clients, if h.clients == 1 { "" } else { "s" }, wifi::HOTSPOT_IP, h.channel);
        text::draw_fit(fb, Font::Body, &devs, 14, Y0 + 94, 292, FG);
        let web = match (srv.dropbox, srv.opds) {
            (false, false) => "Drop box, books: off (Wi-Fi tools)".to_string(),
            (true, true) => format!("http://{}:8080/  + /opds", wifi::HOTSPOT_IP),
            (true, false) => format!("http://{}:8080/", wifi::HOTSPOT_IP),
            (false, true) => format!("http://{}:8080/opds", wifi::HOTSPOT_IP),
        };
        text::draw_fit(fb, Font::Body, &web, 14, Y0 + 114, 292, if srv.dropbox || srv.opds { CYAN } else { DIM });
        fill(fb, 10, Y0 + 140, 300, 46, PANEL);
        text::draw_fit(fb, Font::BodyBold, "Join from the phone or iPad,", 18, Y0 + 145, 286, FG);
        text::draw_fit(fb, Font::Small, "then open the address above in a browser.", 18, Y0 + 166, 286, DIM);
        widgets::hints(fb, &[("OK", if reveal { "hide" } else { "show" }), ("Tri", "new password"), ("Lt/X", "back")]);
    } else {
        text::draw(fb, Font::Huge, "Hotspot off", 14, Y0 + 8, FG);
        let name = h.map(|h| h.ssid.clone()).unwrap_or_else(|| wifi::hotspot_ssid(zpod_hal::sys::mac_address("wlan0").as_deref()));
        text::draw(fb, Font::Body, "Mode > Hotspot: ZPOD becomes the network", 14, Y0 + 42, FG);
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
        text::draw_fit(fb, Font::Small, "and a hotspot at once. Mode > Client rejoins.", 18, Y0 + 156, 286, DIM);
        text::draw_fit(fb, Font::Small, "USB keeps working.", 18, Y0 + 170, 286, DIM);
        widgets::hints(fb, &[("", "Wi-Fi > Mode > Hotspot turns it on"), ("Lt/X", "back")]);
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

// ------------------------------------------------------------------ test3

fn mode_colour(m: Mode) -> Rgb565 {
    match m {
        Mode::Off => FAINT,
        Mode::Client => CYAN,
        Mode::Hotspot => ACCENT,
    }
}

fn pill(fb: &mut Framebuffer, x: i32, y: i32, s: &str, bg: Rgb565) -> i32 {
    let w = text::width(Font::BodyBold, s) + 12;
    let _ = RoundedRectangle::with_equal_corners(rect(x, y, w as u32, 16), Size::new(5, 5)).into_styled(PrimitiveStyle::with_fill(bg)).draw(fb);
    text::draw(fb, Font::BodyBold, s, x + 6, y + 2, BLACK);
    w
}

/// dBm -> "-52 dBm good"
pub fn dbm_text(d: i32) -> String {
    let q = match crate::tools::bars(d as f32) {
        4 => "excellent",
        3 => "good",
        2 => "fair",
        1 => "weak",
        _ => "very weak",
    };
    format!("{d} dBm {q}")
}

/// Wi-Fi primary screen: status card (mode, SSID, IP, signal, channel)
/// and the rows for this mode.
pub fn draw_wifi(fb: &mut Framebuffer, w: &wifitools::WifiTools, rows: &[WifiRow], ls: &widgets::ListState, saved_n: usize, busy: Option<&str>) {
    clear(fb);
    const CARD: i32 = 80;
    fill(fb, 6, Y0 + 4, 308, (CARD - 8) as u32, PANEL);
    let Some(st) = &w.status else {
        text::draw(fb, Font::Body, "Reading Wi-Fi status...", 16, Y0 + 30, DIM);
        return;
    };
    let px = pill(fb, 14, Y0 + 10, st.mode.label(), mode_colour(st.mode));
    let x = 14 + px + 8;
    let (head, l2, l3): (String, String, String) = match st.mode {
        Mode::Off => ("Wi-Fi is off".into(), "Radio off: no client, no hotspot.".into(), "USB networking still works.".into()),
        Mode::Client => match (&st.link, &st.ip) {
            (Some(l), ip) => (
                l.ssid.clone(),
                format!("{}   ch {}", ip.clone().unwrap_or_else(|| "getting address...".into()), zpod_hal::scan::freq_to_channel(l.freq_mhz)),
                format!("{}{}", dbm_text(l.signal_dbm), l.tx_mbit.map(|r| format!("   {r:.0} Mbit/s")).unwrap_or_default()),
            ),
            (None, Some(ip)) => ("Connected".into(), ip.clone(), String::new()),
            (None, None) => ("Not connected".into(), "Looking for saved networks...".into(), "Join one: Networks nearby".into()),
        },
        Mode::Hotspot => {
            let h = st.hotspot.as_ref();
            let n = st.clients.len();
            (
                h.map(|h| h.ssid.clone()).unwrap_or_else(|| "zpod hotspot".into()),
                format!("{}   ch {}", wifi::HOTSPOT_IP, h.map(|h| h.channel).unwrap_or(0)),
                format!("{n} device{} connected", if n == 1 { "" } else { "s" }),
            )
        }
    };
    text::draw_fit(fb, Font::Title, &head, x, Y0 + 9, 306 - x, FG);
    text::draw_fit(fb, Font::Body, &l2, 14, Y0 + 32, 292, FG);
    text::draw_fit(fb, Font::Small, &l3, 14, Y0 + 50, 292, DIM);
    if let Some(m) = &st.mac {
        text::draw_aligned(fb, Font::Tiny, m, 306, Y0 + 62, Align::Right, FAINT);
    }
    let labels: Vec<(String, String)> = rows
        .iter()
        .map(|r| match r {
            WifiRow::Mode => ("Mode".to_string(), format!("{} >", st.mode.label())),
            WifiRow::Networks => ("Networks nearby".to_string(), ">".to_string()),
            WifiRow::Saved => ("Saved networks".to_string(), format!("{saved_n} >")),
            WifiRow::Devices => ("Connected devices".to_string(), format!("{} >", st.clients.len())),
            WifiRow::Password => ("Password".to_string(), "show >".to_string()),
            WifiRow::Tools => ("Wi-Fi tools".to_string(), ">".to_string()),
        })
        .collect();
    let area = rect(0, Y0 + CARD, 320, (H - CARD) as u32);
    widgets::draw_list(fb, area, 30, ls, labels.len(), |i| widgets::Row { title: &labels[i].0, sub: None, right: Some(&labels[i].1), icon: None, mark: None });
    if let Some(b) = busy {
        widgets::toast(fb, b);
    }
    widgets::hints(fb, &[("OK", "open"), ("Lt/X", "back"), ("Home", "top")]);
}

/// Wi-Fi > Mode: Off / Client / Hotspot.
pub fn draw_mode(fb: &mut Framebuffer, cur: Mode, ls: &widgets::ListState, busy: Option<&str>) {
    let subs = ["Radio off. Saves power; USB still works.", "Join saved networks (home Wi-Fi)", "ZPOD is the network: phones/iPads join it"];
    let area = rect(0, Y0, 320, H as u32);
    widgets::draw_list(fb, area, 40, ls, 3, |i| {
        let m = Mode::ALL[i];
        widgets::Row { title: m.label(), sub: Some(subs[i]), right: if m == cur { Some("current") } else { None }, icon: None, mark: Some(if m == cur { mode_colour(m) } else { BG }) }
    });
    text::draw_fit(fb, Font::Small, "The radio is a client or a hotspot, not both.", 12, Y0 + 132, 296, FAINT);
    text::draw_fit(fb, Font::Small, "Leaving Hotspot: pick Client or Off here.", 12, Y0 + 146, 296, FAINT);
    if let Some(b) = busy {
        widgets::toast(fb, b);
    }
    widgets::hints(fb, &[("OK", "switch"), ("Lt/X", "back"), ("Home", "top")]);
}

fn ago(s: u64) -> String {
    if s < 60 {
        format!("{s} s")
    } else if s < 3600 {
        format!("{} min", s / 60)
    } else {
        format!("{} h {} min", s / 3600, s / 60 % 60)
    }
}

/// Hotspot > Connected devices.
pub fn draw_peers(fb: &mut Framebuffer, peers: &[Peer], ssid: &str, ls: &widgets::ListState) {
    let area = rect(0, Y0, 320, H as u32);
    if peers.is_empty() {
        clear(fb);
        text::draw_aligned(fb, Font::Large, "No devices yet", 160, Y0 + 50, Align::Center, DIM);
        text::draw_aligned(fb, Font::Body, &format!("Join \"{ssid}\" on a phone or iPad,"), 160, Y0 + 84, Align::Center, FG);
        text::draw_aligned(fb, Font::Body, "password: Wi-Fi > Password.", 160, Y0 + 102, Align::Center, FG);
    } else {
        let rows: Vec<(String, String, String)> = peers
            .iter()
            .map(|p| {
                (
                    p.name.clone().or(p.ip.clone()).unwrap_or_else(|| "(no address yet)".into()),
                    format!("{}  {}{}", p.ip.clone().unwrap_or_else(|| "--".into()), p.mac, p.connected_s.map(|s| format!("  {}", ago(s))).unwrap_or_default()),
                    p.signal_dbm.map(|d| format!("{d} dBm")).unwrap_or_default(),
                )
            })
            .collect();
        widgets::draw_list(fb, area, 34, ls, rows.len(), |i| widgets::Row { title: &rows[i].0, sub: Some(&rows[i].1), right: Some(&rows[i].2), icon: None, mark: Some(dbm_colour(peers[i].signal_dbm.unwrap_or(-90))) });
    }
    widgets::hints(fb, &[("OK", "refresh"), ("Lt/X", "back"), ("", "every 5 s")]);
}

/// Wi-Fi tools > Devices & addresses: LAN neighbours (ARP) and APs heard.
pub fn draw_addresses(fb: &mut Framebuffer, neigh: &[(String, String)], nets: &[WifiNet], ls: &widgets::ListState, scanning: bool) {
    let mut rows: Vec<(String, String, String, Rgb565)> = neigh.iter().map(|(ip, mac)| (ip.clone(), mac.clone(), "LAN".to_string(), CYAN)).collect();
    for n in nets {
        let name = if n.ssid.is_empty() { "(hidden)".to_string() } else { n.ssid.clone() };
        rows.push((name, format!("{}  ch {}  {}", n.bssid, n.channel, n.security), format!("{:.0} dBm", n.signal_dbm), dbm_colour(n.signal_dbm as i32)));
    }
    let area = rect(0, Y0 + 16, 320, (H - 16) as u32);
    clear(fb);
    text::draw_fit(fb, Font::Small, &format!("{} on this LAN (ARP)  \u{b7}  {} access points{}", neigh.len(), nets.len(), if scanning { "  scanning..." } else { "" }), 10, Y0 + 3, 300, DIM);
    widgets::draw_list(fb, area, 34, ls, rows.len(), |i| widgets::Row { title: &rows[i].0, sub: Some(&rows[i].1), right: Some(&rows[i].2), icon: None, mark: Some(rows[i].3) });
    widgets::hints(fb, &[("OK", "rescan"), ("Lt/X", "back"), ("", "passive: nothing is probed")]);
}

const CURVE_COLOURS: [Rgb565; 8] = [rgb(255, 196, 0), rgb(0, 216, 120), rgb(255, 120, 200), rgb(160, 140, 255), rgb(255, 150, 60), rgb(120, 230, 230), rgb(200, 220, 90), rgb(240, 100, 100)];

/// Wi-Fi tools > Channel utilization: 2.4 GHz axis 1-13, one curve per
/// network spanning its ~22 MHz (about +/-2 channels), height by RSSI;
/// shading under the axis = how busy each channel is (overlap-weighted);
/// a green marker on the least busy of 1/6/11; yours in cyan.
pub fn draw_chan_util(fb: &mut Framebuffer, nets: &[WifiNet], scanning: bool, age: &str) {
    clear(fb);
    let chans = wifi::congestion(nets);
    let best = wifi::best_channel(&chans);
    let two4: Vec<&WifiNet> = nets.iter().filter(|n| (1..=14).contains(&n.channel) && n.freq_mhz < 2500).collect();
    // x: channel -1.5 .. 15.5 across 30..314 ; y: -95 dBm at base, -30 at top
    let (xl, xr) = (30, 314);
    let xof = |c: f32| xl + ((c + 1.5) / 17.0 * (xr - xl) as f32) as i32;
    let (top, base) = (Y0 + 22, Y0 + 142);
    let yof = |d: f32| base - (((d.clamp(-95.0, -30.0) + 95.0) / 65.0) * (base - top) as f32) as i32;
    // busy shading per channel (column behind the curves)
    let maxload = chans.iter().map(|c| c.load).fold(0.0f32, f32::max).max(1.0);
    for c in &chans {
        let a = (c.load / maxload).clamp(0.0, 1.0);
        if a > 0.05 {
            let col = rgb((20.0 + 70.0 * a) as u8, (22.0 + 10.0 * a) as u8, (48.0 - 10.0 * a) as u8);
            let (x0, x1) = (xof(c.ch as f32 - 0.5), xof(c.ch as f32 + 0.5));
            fill(fb, x0, top, (x1 - x0) as u32, (base - top) as u32, col);
        }
    }
    for (d, l) in [(-30, "-30"), (-50, "-50"), (-70, "-70"), (-90, "-90")] {
        let y = yof(d as f32);
        line(fb, (xl, y), (xr, y), PANEL, 1);
        text::draw_aligned(fb, Font::Tiny, l, xl - 3, y - 4, Align::Right, FAINT);
    }
    // curves: weakest first so strong ones are on top
    let mut order: Vec<usize> = (0..two4.len()).collect();
    order.sort_by(|a, b| two4[*a].signal_dbm.partial_cmp(&two4[*b].signal_dbm).unwrap_or(std::cmp::Ordering::Equal));
    let mut labels: Vec<(i32, i32, String, Rgb565)> = Vec::new();
    for (k, &i) in order.iter().enumerate() {
        let n = two4[i];
        let col = if n.associated { CYAN } else { CURVE_COLOURS[i % CURVE_COLOURS.len()] };
        let c = n.channel as f32;
        let peak = yof(n.signal_dbm);
        let mut prev: Option<(i32, i32)> = None;
        for s in 0..=24 {
            let t = s as f32 / 24.0; // 0..1 across +/-2.2 channels
            let cx = c - 2.2 + 4.4 * t;
            let h = (std::f32::consts::PI * t).sin(); // raised arch
            let y = base - ((base - peak) as f32 * h) as i32;
            let p = (xof(cx), y);
            if let Some(q) = prev {
                line(fb, q, p, col, if n.associated { 2 } else { 1 });
            }
            prev = Some(p);
        }
        if k + 6 >= order.len() {
            labels.push((xof(c), peak - 11, if n.ssid.is_empty() { "(hidden)".into() } else { n.ssid.clone() }, col));
        }
    }
    // labels for the strongest few, nudged apart vertically
    labels.sort_by_key(|l| l.1);
    let mut used: Vec<(i32, i32, i32)> = Vec::new();
    for (x, mut y, s, col) in labels {
        let s = text::fit(Font::Tiny, &s, 70);
        let w = text::width(Font::Tiny, &s);
        let x0 = (x - w / 2).clamp(xl, xr - w);
        while used.iter().any(|(ux, uy, uw)| (y - uy).abs() < 9 && x0 < ux + uw && ux < &(x0 + w)) {
            y += 9;
        }
        y = y.max(top - 2);
        text::draw(fb, Font::Tiny, &s, x0, y, col);
        used.push((x0, y, w));
    }
    // axis + channel numbers
    line(fb, (xl, base), (xr, base), FAINT, 1);
    for c in &chans {
        let x = xof(c.ch as f32);
        let lc = if c.ch == best { OK } else if [1, 6, 11].contains(&c.ch) { FG } else { DIM };
        text::draw_aligned(fb, Font::Small, &c.ch.to_string(), x, base + 4, Align::Center, lc);
    }
    // best-channel marker under the number
    let bx = xof(best as f32);
    let _ = Triangle::new(Point::new(bx, base + 17), Point::new(bx - 5, base + 25), Point::new(bx + 5, base + 25)).into_styled(PrimitiveStyle::with_fill(OK)).draw(fb);
    let n = two4.len();
    if scanning && nets.is_empty() {
        text::draw(fb, Font::Body, "Scanning...", 12, Y0 + 4, DIM);
    } else {
        text::draw(fb, Font::Body, &format!("{n} networks on 2.4 GHz"), 12, Y0 + 4, FG);
        text::draw_aligned(fb, Font::BodyBold, &format!("least busy: {best}"), 308, Y0 + 4, Align::Right, OK);
    }
    text::draw_fit(fb, Font::Small, "curve = network (22 MHz), higher = stronger", 12, base + 29, 296, DIM);
    text::draw_fit(fb, Font::Small, "shade = busy   cyan = yours   green = best 1/6/11", 12, base + 41, 296, FAINT);
    widgets::hints(fb, &[("OK", if scanning { "scanning" } else { "rescan" }), ("Lt/X", "back"), ("", age)]);
}

/// Tools > USB: gadget status and what we can see of the host.
pub fn draw_usb(fb: &mut Framebuffer, u: &usb::Info) {
    clear(fb);
    let up = u.link_up;
    let px = pill(fb, 14, Y0 + 10, if up { "Connected" } else if u.udc_state.as_deref() == Some("configured") { "Configured" } else { "Not connected" }, if up { OK } else { FAINT });
    text::draw_fit(fb, Font::Title, &u.function.clone().unwrap_or_else(|| "USB gadget".into()), 14 + px + 8, Y0 + 9, 290 - px, FG);
    let mut y = Y0 + 36;
    let mut kv = |k: &str, v: &str, c: Rgb565| {
        text::draw(fb, Font::Small, k, 14, y + 3, DIM);
        text::draw_fit(fb, Font::Body, v, 96, y, 216, c);
        y += 20;
    };
    let dash = |o: &Option<String>| o.clone().unwrap_or_else(|| "--".into());
    kv("Controller", &format!("{}{}", dash(&u.udc_state), u.speed.as_ref().map(|s| format!("  {s}")).unwrap_or_default()), FG);
    kv("usb0", &format!("{}  {}", if up { "up" } else { "down" }, dash(&u.ip)), if up { FG } else { DIM });
    kv("Host", &u.host_ip.as_ref().map(|ip| format!("{ip}  {}", u.host_mac.clone().unwrap_or_default())).unwrap_or_else(|| "not seen yet".into()), if u.host_ip.is_some() { CYAN } else { DIM });
    kv("Host name", &dash(&u.host_name), FG);
    kv("Traffic", &format!("rx {}  tx {}", zpod_hal::storage::human(u.rx_bytes), zpod_hal::storage::human(u.tx_bytes)), FG);
    kv("Login", &format!("ssh zpod@{}", u.ip.clone().unwrap_or_else(|| "10.12.194.1".into())), DIM);
    fill(fb, 10, Y0 + 162, 300, 24, PANEL);
    text::draw_fit(fb, Font::Small, "Bash Bunny-style tools: coming later.", 18, Y0 + 168, 286, FAINT);
    widgets::hints(fb, &[("OK", "refresh"), ("Lt/X", "back"), ("Home", "top")]);
}
