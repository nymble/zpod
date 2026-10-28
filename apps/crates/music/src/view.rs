//! Music app rendering (320x240, status bar on top, button hints below).
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use embedded_graphics::{
    prelude::*,
    primitives::{Circle, PrimitiveStyle, RoundedRectangle},
};
use zpod_core::{PlayState, StatusInfo};
use zpod_ui::{
    fill, rect,
    status::draw_status_bar,
    text::{self, Align},
    theme,
    widgets::{self, draw_list, Row},
    Canvas, Font,
};

use crate::app::{Browse, MusicApp, Screen, MENU};
use crate::model::{fmt_time, plural, with_year, RepeatMode};

const BODY_Y: i32 = theme::STATUS_H;
const BODY_H: i32 = 240 - theme::STATUS_H - widgets::HINTS_H;
pub const ART: u32 = 120;
const PROG_Y: i32 = 162;

pub fn title(app: &MusicApp) -> String {
    match &app.top().screen {
        Screen::Menu => "Music".into(),
        Screen::Artists(Browse::Artists, v) => format!("Artists ({})", v.len()),
        Screen::Artists(Browse::Albums, v) => {
            let n: u32 = v.iter().map(|a| a.albums).sum();
            format!("Albums ({n}) by artist")
        }
        Screen::Albums(Some(a), v) => format!("{a} ({})", v.len()),
        Screen::Albums(None, v) => format!("Albums ({})", v.len()),
        Screen::Tracks(t, v) => format!("{t} \u{b7} {}", v.len()),
        Screen::Playlists(_) => "Playlists".into(),
        Screen::Queue(q) => format!("Queue ({})", q.len()),
        Screen::NowPlaying => "Playing".into(),
    }
}

fn two_line(screen: &Screen) -> bool {
    matches!(screen, Screen::Albums(..) | Screen::Tracks(..) | Screen::Queue(_))
}

/// Paint what the app marked as changed. `status_changed`: status bar data
/// differs from the last paint.
pub fn render<D: Canvas>(app: &mut MusicApp, d: &mut D, si: &StatusInfo, status_changed: bool) {
    let row_h = if two_line(&app.top().screen) { 34 } else { 29 };
    app.visible_rows = (BODY_H / row_h) as usize;
    if app.need_full {
        draw_status_bar(d, &title(app), si);
        body(app, d, row_h);
        hints(app, d);
        if let Some((msg, _)) = &app.toast {
            widgets::toast(d, msg);
        }
        app.need_full = false;
        app.need_progress = false;
        return;
    }
    if status_changed {
        draw_status_bar(d, &title(app), si);
    }
    if app.need_progress && app.top().screen == Screen::NowPlaying && app.toast.is_none() {
        progress(app, d);
        app.need_progress = false;
    }
}

fn hints<D: Canvas>(app: &MusicApp, d: &mut D) {
    match &app.top().screen {
        Screen::NowPlaying => widgets::hints(d, &[("< >", "prev/next"), ("Up", "repeat"), ("Down", "shuffle"), ("OK", "queue"), ("X", "back")]),
        Screen::Menu => widgets::hints(d, &[("OK", "open"), ("Tri", "now playing"), ("Play", "pause"), ("Home", "menu")]),
        Screen::Queue(_) => widgets::hints(d, &[("OK", "play"), ("Sq", "remove"), ("X", "back"), ("Tri", "now playing")]),
        Screen::Tracks(..) => widgets::hints(d, &[("OK", "play"), ("Sq", "+queue"), ("X", "back"), ("Tri", "now playing")]),
        _ => widgets::hints(d, &[("OK", "open"), ("Sq", "+queue"), ("X", "back"), ("Tri", "now playing")]),
    }
}

fn body<D: Canvas>(app: &MusicApp, d: &mut D, row_h: i32) {
    let area = rect(0, BODY_Y, 320, BODY_H as u32);
    if let Some(err) = &app.error {
        let _ = d.fill_solid(&area, theme::BG);
        text::draw_aligned(d, Font::Title, "Music service unavailable", 160, 60, Align::Center, theme::WARN);
        text::draw_aligned(d, Font::Small, &text::fit(Font::Small, err, 300), 160, 90, Align::Center, theme::DIM);
        text::draw_aligned(d, Font::Body, "Is mpd running?  sudo systemctl status mpd", 160, 122, Align::Center, theme::FG);
        text::draw_aligned(d, Font::Small, "Retrying every few seconds...", 160, 150, Align::Center, theme::DIM);
        return;
    }
    let lv = app.top();
    let playing_uri = app.current.as_ref().map(|t| t.uri.as_str());
    let cur_pos = app.status.as_ref().and_then(|s| s.pos);
    let len = MusicApp::len_of(&lv.screen);
    match &lv.screen {
        Screen::NowPlaying => now_playing(app, d),
        Screen::Menu => {
            let qlen = app.status.as_ref().map(|s| format!("{}", s.queue_len));
            let st = app.stats.filter(|s| s.songs > 0);
            let artists = st.map(|s| format!("{}", s.artists));
            let albums = st.map(|s| format!("{}", s.albums));
            draw_list(d, area, row_h, &lv.ls, len, |i| Row {
                title: MENU[i],
                sub: None,
                right: match MENU[i] {
                    "Queue" => qlen.as_deref(),
                    "Artists" => artists.as_deref(),
                    "Albums" => albums.as_deref(),
                    _ => None,
                },
                icon: None,
                mark: if i == 0 && app.status.as_ref().is_some_and(|s| s.state == PlayState::Playing) { Some(theme::OK) } else { None },
            })
        }
        Screen::Playlists(v) => draw_list(d, area, row_h, &lv.ls, len, |i| Row::simple(&v[i])),
        Screen::Artists(_, v) => {
            let counts: Vec<String> = v.iter().map(|a| if a.albums > 0 { plural(a.albums, "album") } else { String::new() }).collect();
            draw_list(d, area, row_h, &lv.ls, len, |i| Row {
                title: &v[i].name,
                sub: None,
                right: Some(counts[i].as_str()).filter(|c| !c.is_empty()),
                icon: None,
                mark: None,
            })
        }
        Screen::Albums(artist, v) => {
            let off = artist.is_some() as usize;
            let years: Vec<String> = v.iter().map(|a| a.year.map(|y| format!("{y}")).unwrap_or_default()).collect();
            let subs: Vec<String> = v
                .iter()
                .map(|a| match (artist.is_none(), a.summary()) {
                    (true, s) if s.is_empty() => a.artist.clone(),
                    (true, s) => format!("{} \u{b7} {s}", a.artist),
                    (false, s) => s,
                })
                .collect();
            let all = format!("{} \u{b7} {}", plural(v.len() as u32, "album"), plural(v.iter().map(|a| a.tracks).sum(), "track"));
            draw_list(d, area, row_h, &lv.ls, len, |i| {
                if off == 1 && i == 0 {
                    return Row { title: "All tracks", sub: Some(all.as_str()), right: None, icon: None, mark: Some(theme::ACCENT) };
                }
                let j = i - off;
                Row {
                    title: &v[j].album,
                    sub: Some(subs[j].as_str()).filter(|s| !s.is_empty()),
                    right: Some(years[j].as_str()).filter(|s| !s.is_empty()),
                    icon: None,
                    mark: None,
                }
            })
        }
        Screen::Tracks(_, v) => {
            let durs: Vec<String> = v.iter().map(|t| fmt_time(t.duration)).collect();
            let subs: Vec<String> = v
                .iter()
                .map(|t| match t.track {
                    Some(n) => format!("{n}. {}", t.artist),
                    None => t.artist.clone(),
                })
                .collect();
            draw_list(d, area, row_h, &lv.ls, len, |i| Row {
                title: &v[i].title,
                sub: Some(&subs[i]),
                right: Some(&durs[i]),
                icon: None,
                mark: if Some(v[i].uri.as_str()) == playing_uri { Some(theme::OK) } else { None },
            })
        }
        Screen::Queue(v) => {
            let durs: Vec<String> = v.iter().map(|t| fmt_time(t.duration)).collect();
            let subs: Vec<String> = v.iter().map(|t| format!("{} \u{b7} {}", t.artist, t.album)).collect();
            draw_list(d, area, row_h, &lv.ls, len, |i| Row {
                title: &v[i].title,
                sub: Some(&subs[i]),
                right: Some(&durs[i]),
                icon: None,
                mark: if Some(i) == cur_pos { Some(theme::OK) } else { None },
            });
            if v.is_empty() {
                text::draw_aligned(d, Font::Small, "Square on a track or album adds it here", 160, 150, Align::Center, theme::DIM);
            }
        }
    }
}

/// Wrap into at most two lines that fit `w`.
fn wrap2(font: Font, s: &str, w: i32) -> (String, Option<String>) {
    if text::width(font, s) <= w {
        return (s.into(), None);
    }
    let mut first = String::new();
    let mut rest_at = 0;
    for (i, _) in s.match_indices(' ') {
        if text::width(font, &s[..i]) <= w {
            first = s[..i].into();
            rest_at = i + 1;
        } else {
            break;
        }
    }
    if first.is_empty() {
        return (text::fit(font, s, w), None);
    }
    (first, Some(text::fit(font, &s[rest_at..], w)))
}

fn note_placeholder<D: Canvas>(d: &mut D, x: i32, y: i32) {
    let _ = RoundedRectangle::with_equal_corners(rect(x, y, ART, ART), Size::new(8, 8))
        .into_styled(PrimitiveStyle::with_fill(theme::PANEL))
        .draw(d);
    // a quaver
    let c = theme::FAINT;
    let _ = Circle::new(Point::new(x + 34, y + 70), 26).into_styled(PrimitiveStyle::with_fill(c)).draw(d);
    let _ = Circle::new(Point::new(x + 70, y + 60), 26).into_styled(PrimitiveStyle::with_fill(c)).draw(d);
    fill(d, x + 55, y + 26, 5, 58, c);
    fill(d, x + 91, y + 16, 5, 58, c);
    fill(d, x + 55, y + 16, 41, 10, c);
}

fn now_playing<D: Canvas>(app: &MusicApp, d: &mut D) {
    let _ = d.fill_solid(&rect(0, BODY_Y, 320, BODY_H as u32), theme::BG);
    let (ax, ay) = (10, BODY_Y + 10);
    match app.art.as_ref().and_then(|(_, a)| a.as_ref()) {
        Some(img) if app.current.is_some() => img.draw(d, ax + (ART as i32 - img.w as i32) / 2, ay + (ART as i32 - img.h as i32) / 2),
        _ => note_placeholder(d, ax, ay),
    }
    let tx = ax + ART as i32 + 12;
    let tw = 320 - tx - 8;
    let st = app.status.clone().unwrap_or_default();
    let Some(t) = &app.current else {
        text::draw(d, Font::Large, "Nothing playing", tx, ay + 6, theme::FG);
        text::draw(d, Font::Small, "Pick music in Artists,", tx, ay + 40, theme::DIM);
        text::draw(d, Font::Small, "Albums or Playlists,", tx, ay + 56, theme::DIM);
        text::draw(d, Font::Small, "or choose Shuffle all.", tx, ay + 72, theme::DIM);
        return;
    };
    let (l1, l2) = wrap2(Font::Large, &t.title, tw);
    let mut y = ay + 2;
    text::draw(d, Font::Large, &l1, tx, y, theme::WHITE);
    y += 22;
    if let Some(l2) = l2 {
        text::draw(d, Font::Large, &l2, tx, y, theme::WHITE);
        y += 22;
    }
    y += 4;
    text::draw_fit(d, Font::BodyBold, &t.artist, tx, y, tw, theme::ACCENT);
    y += 18;
    text::draw_fit(d, Font::Small, &with_year(&t.album, t.year), tx, y, tw, theme::DIM);
    y += 16;
    if let Some(p) = st.pos {
        let s = format!("Track {} of {}", p + 1, st.queue_len);
        text::draw(d, Font::Small, &s, tx, y, theme::DIM);
    }
    // mode badges
    let mut bx = tx;
    let by = ay + ART as i32 - 16;
    let mut badge = |d: &mut D, label: &str, c| {
        let w = text::width(Font::Tiny, label) + 10;
        let _ = RoundedRectangle::with_equal_corners(rect(bx, by, w as u32, 14), Size::new(4, 4))
            .into_styled(PrimitiveStyle::with_fill(c))
            .draw(d);
        text::draw(d, Font::Tiny, label, bx + 5, by + 3, theme::BLACK);
        bx += w + 5;
    };
    match st.state {
        PlayState::Paused => badge(d, "PAUSED", theme::ACCENT),
        PlayState::Stopped => badge(d, "STOPPED", theme::DIM),
        _ => {}
    }
    match st.repeat {
        RepeatMode::All => badge(d, "REPEAT", theme::CYAN),
        RepeatMode::One => badge(d, "REPEAT 1", theme::CYAN),
        RepeatMode::Off => {}
    }
    if st.random {
        badge(d, "SHUFFLE", theme::OK);
    }
    progress(app, d);
}

fn progress<D: Canvas>(app: &MusicApp, d: &mut D) {
    let st = app.status.clone().unwrap_or_default();
    fill(d, 0, PROG_Y - 4, 320, 40, theme::BG);
    if app.current.is_none() {
        return;
    }
    let frac = if st.duration > 0.0 { st.elapsed / st.duration } else { 0.0 };
    let _ = RoundedRectangle::with_equal_corners(rect(10, PROG_Y, 300, 6), Size::new(3, 3))
        .into_styled(PrimitiveStyle::with_fill(theme::FAINT))
        .draw(d);
    let fw = (300.0 * frac.clamp(0.0, 1.0)) as u32;
    if fw > 0 {
        let _ = RoundedRectangle::with_equal_corners(rect(10, PROG_Y, fw.max(6), 6), Size::new(3, 3))
            .into_styled(PrimitiveStyle::with_fill(if st.state == PlayState::Playing { theme::OK } else { theme::ACCENT }))
            .draw(d);
    }
    text::draw(d, Font::Small, &fmt_time(st.elapsed), 10, PROG_Y + 12, theme::FG);
    if st.duration > 0.0 {
        let rem = format!("-{}", fmt_time(st.duration - st.elapsed));
        text::draw_aligned(d, Font::Small, &rem, 310, PROG_Y + 12, Align::Right, theme::DIM);
        text::draw_aligned(d, Font::Small, &fmt_time(st.duration), 160, PROG_Y + 12, Align::Center, theme::FAINT);
    }
}
