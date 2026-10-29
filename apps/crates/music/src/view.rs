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

use crate::app::{Browse, MusicApp, Screen, MENU, PLAYLIST_ACTIONS};
use crate::model::{fmt_time, new_playlist_name, plural, with_year, RepeatMode};

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
        Screen::PlaylistMenu(n) => n.clone(),
        Screen::AddTo(what, _, _) => format!("Add {} to playlist", what.to_lowercase()),
        Screen::ConfirmDelete(_) => "Delete playlist".into(),
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
        if let Some((c, _)) = app.jump {
            jump_letter(d, c);
        }
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

/// Big index letter while fast-scrolling by letter.
fn jump_letter<D: Canvas>(d: &mut D, c: char) {
    let r = rect(250, 92, 56, 56);
    let _ = RoundedRectangle::with_equal_corners(r, Size::new(8, 8))
        .into_styled(PrimitiveStyle::with_fill(theme::PANEL))
        .draw(d);
    let _ = RoundedRectangle::with_equal_corners(r, Size::new(8, 8))
        .into_styled(PrimitiveStyle::with_stroke(theme::ACCENT, 2))
        .draw(d);
    let mut b = [0u8; 4];
    text::draw_aligned(d, Font::Huge, c.encode_utf8(&mut b), 278, 106, Align::Center, theme::ACCENT);
}

fn hints<D: Canvas>(app: &MusicApp, d: &mut D) {
    match &app.top().screen {
        Screen::NowPlaying => widgets::hints(d, &[("< >", "prev/next"), ("OK", "queue"), ("Sq", "+playlist"), ("Tri", "album+pl"), ("X", "back")]),
        Screen::Artists(..) | Screen::Playlists(_) => widgets::hints(d, &[("OK", "open"), ("Sq", "+queue"), ("Hold Up/Dn", "A-Z"), ("X", "back")]),
        Screen::PlaylistMenu(_) => widgets::hints(d, &[("OK", "choose"), ("Sq", "+queue"), ("X", "back"), ("Tri", "now playing")]),
        Screen::AddTo(..) => widgets::hints(d, &[("OK", "add here"), ("X", "cancel")]),
        Screen::ConfirmDelete(_) => widgets::hints(d, &[("OK", "delete"), ("X", "cancel")]),
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
        Screen::Playlists(v) => {
            draw_list(d, area, row_h, &lv.ls, len, |i| Row::simple(&v[i]));
            if v.is_empty() {
                text::draw_aligned(d, Font::Small, "In Now Playing: Square adds the track,", 160, 140, Align::Center, theme::DIM);
                text::draw_aligned(d, Font::Small, "Triangle the album, to a playlist", 160, 154, Align::Center, theme::DIM);
            }
        }
        Screen::PlaylistMenu(_) => draw_list(d, area, row_h, &lv.ls, len, |i| Row {
            title: PLAYLIST_ACTIONS[i],
            sub: None,
            right: None,
            icon: None,
            mark: if PLAYLIST_ACTIONS[i].starts_with("Delete") { Some(theme::WARN) } else { None },
        }),
        Screen::AddTo(_, _, names) => {
            let fresh = new_playlist_name(names);
            draw_list(d, area, row_h, &lv.ls, len, |i| {
                if i == 0 {
                    Row { title: "New playlist", sub: None, right: Some(fresh.as_str()), icon: None, mark: Some(theme::ACCENT) }
                } else {
                    Row::simple(&names[i - 1])
                }
            })
        }
        Screen::ConfirmDelete(name) => {
            let _ = d.fill_solid(&area, theme::BG);
            let n = text::fit(Font::Body, name, 260);
            widgets::dialog(d, "Delete?", &[n.as_str(), "The music files stay.", "OK = delete    X = cancel"], theme::WARN);
        }
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
    progress(app, d);
    controls(d, &st);
}

/// Shuffle and repeat controls under the progress bar (Up = repeat,
/// Down = shuffle). Always visible; lit when on.
fn controls<D: Canvas>(d: &mut D, st: &crate::model::PlayerStatus) {
    let y = 200;
    fill(d, 0, y - 2, 320, 22, theme::BG);
    let rep = match st.repeat {
        RepeatMode::Off => "Repeat off",
        RepeatMode::All => "Repeat all",
        RepeatMode::One => "Repeat one",
    };
    let shuf = if st.random { "Shuffle on" } else { "Shuffle off" };
    let pills = [(10, "Up", rep, st.repeat != RepeatMode::Off, theme::CYAN), (165, "Dn", shuf, st.random, theme::OK)];
    for (x, key, label, on, c) in pills {
        let _ = RoundedRectangle::with_equal_corners(rect(x, y, 145, 18), Size::new(5, 5))
            .into_styled(PrimitiveStyle::with_fill(if on { c } else { theme::PANEL }))
            .draw(d);
        let kw = text::width(Font::Tiny, key) + 6;
        let _ = RoundedRectangle::with_equal_corners(rect(x + 4, y + 3, kw as u32, 12), Size::new(3, 3))
            .into_styled(PrimitiveStyle::with_fill(if on { theme::BLACK } else { theme::FAINT }))
            .draw(d);
        text::draw(d, Font::Tiny, key, x + 7, y + 5, theme::WHITE);
        text::draw(d, Font::BodyBold, label, x + kw + 10, y + 3, if on { theme::BLACK } else { theme::DIM });
    }
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
