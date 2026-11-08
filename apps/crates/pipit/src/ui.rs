//! Pipit screens (320×240): header, 5×16 grid, mode panel, actions, help.

use embedded_graphics::{pixelcolor::Rgb565, prelude::*, primitives::{PrimitiveStyle, RoundedRectangle}};
use zpod_core::StatusInfo;
use zpod_seq::demos::DEMOS;
use zpod_seq::pattern::{Rate, ECHO_TIMES, ROOT_NAMES, MAX_STEPS, TRACKS};
use zpod_ui::{fill, rect, status::draw_status_bar, text, text::Align, theme::*, Font, Framebuffer};

use crate::editor::{Action, Editor, Lane, Mode, KITS, LIVE_LANE0, OUTPUTS};

pub const TRACK_NAMES: [&str; TRACKS] = ["KICK", "SNARE", "HAT", "BASS", "LEAD"];
const TRACK_COL: [Rgb565; TRACKS] = [rgb(255, 110, 70), rgb(255, 196, 0), rgb(120, 220, 255), rgb(150, 120, 255), rgb(80, 230, 150)];
const BG2: Rgb565 = rgb(14, 17, 36);
const BEAT: Rgb565 = rgb(22, 26, 50);
const CELL_OFF: Rgb565 = rgb(36, 40, 66);
const GX: i32 = 46;
const GY: i32 = 42;
const CW: i32 = 17;
const RH: i32 = 19;

fn rr(fb: &mut Framebuffer, x: i32, y: i32, w: i32, h: i32, r: u32, c: Rgb565) {
    let _ = RoundedRectangle::with_equal_corners(rect(x, y, w as u32, h as u32), Size::new(r, r)).into_styled(PrimitiveStyle::with_fill(c)).draw(fb);
}
fn frame(fb: &mut Framebuffer, x: i32, y: i32, w: i32, h: i32, c: Rgb565, t: u32) {
    let _ = RoundedRectangle::with_equal_corners(rect(x, y, w as u32, h as u32), Size::new(3, 3)).into_styled(PrimitiveStyle::with_stroke(c, t)).draw(fb);
}
fn mix(a: Rgb565, b: Rgb565, t: f32) -> Rgb565 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t) as u8;
    Rgb565::new(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

pub fn gate_label(g: u8) -> String {
    match g {
        1 => "1/4 step".into(),
        2 => "1/2 step".into(),
        3 => "3/4 step".into(),
        4 => "1 step".into(),
        g if g % 4 == 0 => format!("{} steps", g / 4),
        g => format!("{:.1} steps", g as f32 / 4.0),
    }
}

/// Short beat-division tag for the header.
pub fn rate_tag(r: Rate) -> &'static str {
    match r {
        Rate::Half => "1/2x",
        Rate::Normal => "1x",
        Rate::Double => "2x",
        Rate::Triplet => "3:2",
    }
}

fn signed(v: i8) -> String {
    if v == 0 { "0".into() } else { format!("{v:+}") }
}

/// Parameters shown per panel page (3 rows × 2 columns).
const PAGE: usize = 6;

pub fn draw(fb: &mut Framebuffer, st: &StatusInfo, e: &Editor, playhead: Option<u8>, toast: Option<&str>, silent: bool) {
    fill(fb, 0, 0, 320, 240, BG);
    draw_status_bar(fb, "Pipit", st);
    // header: mode tabs + transport
    for (i, m) in Mode::ALL.iter().enumerate() {
        let x = 2 + i as i32 * 37;
        let on = *m == e.mode;
        rr(fb, x, 22, 35, 14, 4, if on { SEL } else { PANEL });
        text::draw_aligned(fb, Font::Tiny, m.name(), x + 17, 25, Align::Center, if on { WHITE } else { DIM });
    }
    let tcol = if e.playing { OK } else { DIM };
    text::draw_aligned(fb, Font::Small, &format!("{} {:.0} bpm {}", if e.playing { "PLAY" } else { "STOP" }, e.pat.tempo, rate_tag(e.pat.rate)), 316, 24, Align::Right, tcol);

    // grid
    let len = e.pat.length as usize;
    for (t, tr) in e.pat.tracks.iter().enumerate() {
        let y = GY + t as i32 * RH;
        let col = if e.pat.audible(t) { TRACK_COL[t] } else { FAINT };
        let sel = t == e.track;
        if sel {
            rr(fb, 1, y - 1, 43, RH - 1, 3, PANEL);
        }
        text::draw(fb, Font::Small, TRACK_NAMES[t], 4, y + 3, col);
        if tr.mute {
            text::draw(fb, Font::Tiny, "M", 36, y + 4, WARN);
        } else if tr.solo {
            text::draw(fb, Font::Tiny, "S", 36, y + 4, OK);
        }
        for i in 0..MAX_STEPS {
            let x = GX + i as i32 * CW;
            if t == 0 {
                fill(fb, x - 1, GY - 2, CW as u32, (RH * TRACKS as i32 + 1) as u32, if (i / 4) % 2 == 0 { BEAT } else { BG2 });
            }
            let s = &tr.steps[i];
            let beyond = i >= len;
            if s.on && !beyond {
                let c = mix(CELL_OFF, col, 0.35 + 0.65 * s.vel as f32 / 127.0);
                rr(fb, x, y, CW - 2, RH - 3, 3, c);
                if s.prob < 100 {
                    // chance: hollow centre, bigger hole = less likely
                    let h = 2 + (100 - s.prob as i32) / 20;
                    fill(fb, x + 7 - h, y + 8 - h, (2 * h) as u32, (2 * h - 1) as u32, BG);
                }
                if s.vel >= 110 {
                    fill(fb, x + 3, y + 2, (CW - 8) as u32, 2, WHITE);
                }
            } else {
                rr(fb, x + 4, y + 5, CW - 10, RH - 13, 2, if beyond { BG } else { CELL_OFF });
            }
        }
    }
    // playhead: bar above the grid + column outline (cursor is the white box)
    if let Some(p) = playhead {
        let x = GX + p as i32 * CW;
        fill(fb, x, GY - 5, (CW - 2) as u32, 3, OK);
        frame(fb, x - 1, GY - 2, CW, RH * TRACKS as i32, OK, 1);
    }
    let cx = GX + e.step as i32 * CW;
    let cy = GY + e.track as i32 * RH;
    frame(fb, cx - 2, cy - 2, CW + 2, RH + 1, WHITE, 2);
    for i in [0, 4, 8, 12] {
        text::draw(fb, Font::Tiny, &format!("{}", i + 1), GX + i * CW + 3, GY + RH * TRACKS as i32 + 1, FAINT);
    }

    // panel
    let py = 150;
    rr(fb, 2, py, 316, 88, 6, PANEL);
    let tr = &e.pat.tracks[e.track];
    let s = e.cur();
    match e.mode {
        Mode::Live => draw_live(fb, e, py),
        Mode::Grid => {
            let state = if s.on { "on" } else { "off" };
            text::draw(fb, Font::BodyBold, &format!("{} · step {} · {}", TRACK_NAMES[e.track], e.step + 1, state), 10, py + 6, TRACK_COL[e.track]);
            text::draw(fb, Font::Small, &format!("{}  vel {}  gate {}  chance {}%", e.pat.note_name(e.track, s), s.vel, gate_label(s.gate), s.prob), 10, py + 24, FG);
            let hints = [("Arrows", "move"), ("OK/Cir", "step on/off"), ("Tri", "next mode"), ("Sq", "actions"), ("Play", "start/stop"), ("X", "help")];
            for (i, (k, a)) in hints.iter().enumerate() {
                let x = 10 + (i as i32 % 3) * 104;
                let y = py + 46 + (i as i32 / 3) * 18;
                text::draw(fb, Font::Tiny, k, x, y, CYAN);
                text::draw(fb, Font::Tiny, a, x + 40, y, DIM);
            }
        }
        _ => {
            let title = match e.mode {
                Mode::Step => format!("STEP {} · {}{}", e.step + 1, TRACK_NAMES[e.track], if s.on { "" } else { " (off: OK adds it)" }),
                Mode::Track => format!("TRACK · {}", TRACK_NAMES[e.track]),
                _ => format!("SONG · {}", e.pat.name.as_str()),
            };
            let np = e.mode.params().len();
            let title = if np > PAGE { format!("{title}  {}/{}", e.param / PAGE + 1, np.div_ceil(PAGE)) } else { title };
            text::draw(fb, Font::BodyBold, &title, 10, py + 5, if e.mode == Mode::Song { ACCENT } else { TRACK_COL[e.track] });
            let vals: Vec<String> = match e.mode {
                Mode::Step => vec![e.pat.note_name(e.track, s), format!("{}{}", s.vel, if s.vel >= 110 { " accent" } else { "" }), gate_label(s.gate), format!("{}%", s.prob)],
                Mode::Track => vec![
                    e.bank.find(tr.voice.as_str()).map(|i| e.bank.voices[i].name.as_str().to_string()).unwrap_or_default(),
                    format!("{}", tr.level),
                    if tr.mute { "muted".into() } else { "on".into() },
                    if tr.pitched { format!("{}", tr.octave) } else { "—".into() },
                    signed(tr.macros.tone),
                    signed(tr.macros.decay),
                    if tr.pitched { format!("{} ct", signed(tr.macros.pitch)) } else { format!("{} st", signed(tr.macros.pitch)) },
                    format!("{}", tr.macros.drive),
                    if tr.send == 0 { "off".into() } else { format!("{}", tr.send) },
                ],
                _ => vec![
                    format!("{:.0} bpm", e.pat.tempo),
                    format!("{}%{}", e.pat.swing, if e.pat.swing == 50 { " straight" } else if e.pat.rate == Rate::Triplet { " (off: 3:2)" } else { "" }),
                    e.pat.rate.label().into(),
                    e.pat.scale.name().into(),
                    ROOT_NAMES[e.pat.root as usize % 12].into(),
                    format!("{} steps", e.pat.length),
                    if e.pat.drive == 0 { "off".into() } else { format!("{}", e.pat.drive) },
                    if e.pat.echo == 0 { "off".into() } else { format!("{}", e.pat.echo) },
                    format!("{} note", ECHO_TIMES[e.pat.echo_time as usize % ECHO_TIMES.len()].0),
                    OUTPUTS[e.output].0.into(),
                ],
            };
            let first = e.param / PAGE * PAGE;
            for (i, (name, v)) in e.mode.params().iter().zip(vals.iter()).enumerate().skip(first).take(PAGE) {
                let j = (i - first) as i32;
                let x = 8 + (j % 2) * 156;
                let y = py + 21 + (j / 2) * 21;
                let selp = i == e.param;
                rr(fb, x, y, 150, 19, 4, if selp { SEL } else { BAR });
                text::draw(fb, Font::Tiny, name, x + 6, y + 5, if selp { WHITE } else { DIM });
                text::draw_aligned(fb, Font::Small, v, x + 144, y + 4, Align::Right, FG);
            }
            if e.mode == Mode::Step {
                text::draw_aligned(fb, Font::Tiny, "Up/Dn param · Lt/Rt value · OK on/off · X grid", 160, py + 66, Align::Center, DIM);
            }
        }
    }
    if silent {
        text::draw_aligned(fb, Font::Tiny, "preview: no audio", 312, 154, Align::Right, WARN);
    }
    if let Some(m) = &e.menu {
        draw_menu(fb, e, m);
    }
    if let Some(b) = &e.browser {
        draw_browser(fb, e, b);
    }
    if e.help {
        draw_help(fb);
    }
    if let Some(t) = toast {
        let w = text::width(Font::Small, t) + 20;
        rr(fb, 160 - w / 2, 206, w, 22, 6, BLACK);
        text::draw_aligned(fb, Font::Small, t, 160, 211, Align::Center, ACCENT);
    }
}

fn draw_live(fb: &mut Framebuffer, e: &Editor, py: i32) {
    let rows = [
        ("Tempo", format!("{:.0} bpm", e.pat.tempo)),
        ("Rate", e.pat.rate.label().to_string()),
        ("Swing", if e.pat.rate == Rate::Triplet { format!("{}% (off)", e.pat.swing) } else { format!("{}%", e.pat.swing) }),
    ];
    for (i, (k, v)) in rows.iter().enumerate() {
        let y = py + 5 + i as i32 * 23;
        let sel = e.param == i;
        rr(fb, 8, y, 146, 21, 4, if sel { SEL } else { BAR });
        text::draw(fb, Font::Tiny, k, 14, y + 6, if sel { WHITE } else { DIM });
        text::draw_aligned(fb, if i == 0 { Font::BodyBold } else { Font::Small }, v, 148, y + 4, Align::Right, FG);
    }
    let hint = match e.param {
        0 => "OK: tap tempo · Lt/Rt: ±1",
        1 | 2 => "Lt/Rt: change",
        _ => "Lt mute · Rt solo · OK mute",
    };
    text::draw(fb, Font::Tiny, hint, 12, py + 75, CYAN);
    if e.taps() >= 1 && e.param == 0 {
        text::draw_aligned(fb, Font::Tiny, &format!("taps {}", e.taps()), 150, py + 75, Align::Right, DIM);
    }
    for t in 0..TRACKS {
        let y = py + 4 + t as i32 * 16;
        let sel = e.param == LIVE_LANE0 + t;
        rr(fb, 160, y, 152, 15, 3, if sel { SEL } else { BAR });
        let audible = e.pat.audible(t);
        text::draw(fb, Font::Tiny, TRACK_NAMES[t], 166, y + 3, if audible { TRACK_COL[t] } else { FAINT });
        let (label, c) = match e.lane(t) {
            Lane::Mute => ("MUTE", WARN),
            Lane::Solo => ("SOLO", OK),
            Lane::On if !audible => ("(solo)", FAINT),
            Lane::On => ("on", DIM),
        };
        text::draw_aligned(fb, Font::Tiny, label, 306, y + 3, Align::Right, c);
    }
}

fn draw_browser(fb: &mut Framebuffer, e: &Editor, b: &crate::editor::Browser) {
    let (x, y, w, h) = (14, 24, 292, 214);
    rr(fb, x, y, w, h, 8, BAR);
    frame(fb, x, y, w, h, TRACK_COL[e.track], 2);
    text::draw(fb, Font::Title, &format!("Voice · {}", TRACK_NAMES[e.track]), x + 10, y + 6, TRACK_COL[e.track]);
    text::draw_aligned(fb, Font::Tiny, &format!("{}/{}", b.sel + 1, b.list.len()), x + w - 10, y + 10, Align::Right, DIM);
    let rows = 8usize;
    let first = b.sel.saturating_sub(rows / 2).min(b.list.len().saturating_sub(rows));
    let cur = e.pat.tracks[e.track].voice;
    for (r, &vi) in b.list.iter().enumerate().skip(first).take(rows) {
        let yy = y + 28 + (r - first) as i32 * 18;
        let sel = r == b.sel;
        if sel {
            rr(fb, x + 6, yy - 2, w - 12, 17, 4, SEL);
        }
        let v = &e.bank.voices[vi];
        let info = &e.bank.info[vi];
        text::draw(fb, Font::Tiny, &info.category, x + 12, yy + 2, if sel { WHITE } else { CYAN });
        let mark = if v.id == cur { " *" } else { "" };
        text::draw(fb, Font::Small, &format!("{}{}", v.name.as_str(), mark), x + 62, yy, if sel { WHITE } else { FG });
    }
    let d = &e.bank.info[b.list[b.sel]].desc;
    text::draw(fb, Font::Small, d, x + 12, y + h - 40, FG);
    text::draw_aligned(fb, Font::Tiny, "Up/Dn choose · Rt hear · OK use · X back", x + w / 2, y + h - 16, Align::Center, DIM);
}

fn draw_menu(fb: &mut Framebuffer, e: &Editor, m: &crate::editor::Menu) {
    let (x, y, w, h) = (30, 22, 260, 216);
    rr(fb, x, y, w, h, 8, BAR);
    frame(fb, x, y, w, h, ACCENT, 2);
    text::draw(fb, Font::Title, "Actions", x + 10, y + 6, ACCENT);
    text::draw_aligned(fb, Font::Tiny, &format!("track: {}", TRACK_NAMES[e.track]), x + w - 10, y + 10, Align::Right, DIM);
    for (i, a) in Action::ALL.iter().enumerate() {
        let yy = y + 26 + i as i32 * 19;
        let sel = i == m.sel;
        if sel {
            rr(fb, x + 6, yy - 2, w - 12, 18, 4, SEL);
        }
        let (label, arg) = match a {
            Action::Save => ("Save to slot", format!("< {}{} >", m.slot, if e.slot_used[m.slot as usize - 1] { " used" } else { " empty" })),
            Action::Load => ("Load slot", format!("< {}{} >", m.slot, if e.slot_used[m.slot as usize - 1] { "" } else { " empty" })),
            Action::Demo => ("Load demo", format!("< {} >", DEMOS[m.demo].name)),
            Action::Kit => ("Drum kit", format!("< {} >", KITS[m.kit].0)),
            Action::Euclid => ("Euclid fill track", format!("< {} hits >", m.hits)),
            Action::Notes => ("New notes for track", format!("< seed {} >", m.seed)),
            Action::Clear => ("Clear track", String::new()),
            Action::New => ("New empty pattern", String::new()),
            Action::Undo => ("Undo last action", if e.undo.is_some() { String::new() } else { "(none)".into() }),
        };
        text::draw(fb, Font::Small, label, x + 12, yy + 1, if sel { WHITE } else { FG });
        text::draw_aligned(fb, Font::Small, &arg, x + w - 12, yy + 1, Align::Right, if sel { WHITE } else { CYAN });
    }
    let foot = if m.armed { "OK again overwrites the slot" } else { "Up/Dn choose · Lt/Rt value · OK do it · X close" };
    text::draw_aligned(fb, Font::Tiny, foot, x + w / 2, y + h - 14, Align::Center, if m.armed { WARN } else { DIM });
}

pub fn draw_help(fb: &mut Framebuffer) {
    let (x, y, w, h) = (8, 24, 304, 212);
    rr(fb, x, y, w, h, 8, BAR);
    frame(fb, x, y, w, h, CYAN, 2);
    text::draw(fb, Font::Title, "Pipit buttons", x + 10, y + 6, CYAN);
    text::draw_aligned(fb, Font::Tiny, "any key closes", x + w - 10, y + 10, Align::Right, DIM);
    let rows: [(&str, &str); 12] = [
        ("Play", "start at step 1 / stop"),
        ("Tri", "LIVE>GRID>STEP>TRACK>SONG"),
        ("LIVE", "OK on Tempo = tap; lanes:"),
        ("", "Lt mute, Rt solo, OK mute"),
        ("Arrows", "GRID: move; else Up/Dn, Lt/Rt"),
        ("OK/Cir", "step on/off (TRACK: mute)"),
        ("", "TRACK Voice: OK = browser"),
        ("X", "back to GRID / this help"),
        ("Sq", "save, load, demo, kit, Euclid,"),
        ("", "notes, clear, new, undo"),
        ("Home", "save and exit (music paused)"),
        ("", "green = playhead, white = cursor"),
    ];
    for (i, (k, a)) in rows.iter().enumerate() {
        let yy = y + 28 + i as i32 * 15;
        text::draw(fb, Font::BodyBold, k, x + 12, yy, ACCENT);
        text::draw(fb, Font::Small, a, x + 78, yy + 1, FG);
    }
}
