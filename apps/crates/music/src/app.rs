//! Music app state machine: screens, navigation and button handling.
//!
//! Buttons (vendor ZPOD 2017 conventions where they apply):
//! * Lists: Up/Down move, OK/Circle/Right open or play, X/Left back,
//!   Square add to queue, Triangle now playing.
//! * Now playing: Left previous, Right next (hold = seek +10 s),
//!   Up repeat (off / all / one), Down shuffle on/off, OK queue, X back.
//! * Everywhere: Play/Pause toggles playback, Home returns to the ZPOD menu,
//!   Vol+/- = shared overlay (platform layer). Left at the music menu = exit.
//! * Now Playing overrides Left/Right (previous/next, documented in
//!   docs/ui/BUTTONS-AND-MENUS.md); X = back.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use zpod_core::{Event, Key, KeyState, PlayState};
use zpod_ui::{image::Image565, widgets::ListState};

use crate::model::*;

pub const MENU: [&str; 7] = ["Now Playing", "Artists", "Albums", "Playlists", "Queue", "Shuffle all", "Update library"];

/// Which menu entry an artist list was opened from. Both lead to
/// Artist -> albums; "Albums" just labels the list by album count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Browse {
    Artists,
    Albums,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Screen {
    Menu,
    Artists(Browse, Vec<ArtistRef>),
    /// Albums of an artist (index 0 = "All tracks"), oldest first, or of
    /// the whole library (kept for backends/tests; the menu no longer
    /// opens a flat list).
    Albums(Option<String>, Vec<AlbumRef>),
    Tracks(String, Vec<Track>),
    Playlists(Vec<String>),
    Queue(Vec<Track>),
    NowPlaying,
}

#[derive(Clone, Debug)]
pub struct Level {
    pub screen: Screen,
    pub ls: ListState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Exit,
    Changed,
    Nothing,
}

pub struct MusicApp {
    pub stack: Vec<Level>,
    pub status: Option<PlayerStatus>,
    pub current: Option<Track>,
    /// (album key, art) for the current track.
    pub art: Option<(String, Option<Image565>)>,
    pub toast: Option<(String, u32)>,
    pub error: Option<String>,
    /// Whole screen needs repainting.
    pub need_full: bool,
    /// Only the now-playing progress line changed.
    pub need_progress: bool,
    /// Rows that fit on a list screen (set by the view).
    pub visible_rows: usize,
    /// Library totals shown in the menu (refreshed after a rescan).
    pub stats: Option<LibraryStats>,
}

impl Default for MusicApp {
    fn default() -> Self {
        Self::new()
    }
}

/// Key used to cache album art (one picture per album).
pub fn art_key(t: &Track) -> String {
    if !t.album.is_empty() {
        format!("{}\u{1f}{}", if t.album_artist.is_empty() { &t.artist } else { &t.album_artist }, t.album)
    } else {
        // no album tag: one picture per folder
        t.uri.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default()
    }
}

impl MusicApp {
    pub fn new() -> Self {
        MusicApp {
            stack: vec![Level { screen: Screen::Menu, ls: ListState::default() }],
            status: None,
            current: None,
            art: None,
            toast: None,
            error: None,
            need_full: true,
            need_progress: false,
            visible_rows: 6,
            stats: None,
        }
    }

    pub fn top(&self) -> &Level {
        self.stack.last().unwrap()
    }

    fn top_mut(&mut self) -> &mut Level {
        self.stack.last_mut().unwrap()
    }

    fn push(&mut self, screen: Screen) {
        self.stack.push(Level { screen, ls: ListState::default() });
        self.need_full = true;
    }

    fn pop(&mut self) {
        if self.stack.len() > 1 {
            self.stack.pop();
            self.need_full = true;
        }
    }

    pub fn toast(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), 1500));
        self.need_full = true;
    }

    fn check<T>(&mut self, r: BResult<T>) -> Option<T> {
        match r {
            Ok(v) => {
                if self.error.take().is_some() {
                    self.need_full = true;
                }
                Some(v)
            }
            Err(e) => {
                self.error = Some(e);
                self.need_full = true;
                None
            }
        }
    }

    /// Call once at start: refresh and open Now Playing if music is on.
    pub fn start(&mut self, be: &mut impl MusicBackend) {
        self.poll(be);
        self.refresh_stats(be);
        if self.status.as_ref().is_some_and(|s| s.state == PlayState::Playing) {
            self.push(Screen::NowPlaying);
        }
    }

    fn refresh_stats(&mut self, be: &mut impl MusicBackend) {
        if let Ok(s) = be.stats() {
            if self.stats != Some(s) {
                self.stats = Some(s);
                self.need_full = true;
            }
        }
    }

    pub fn len_of(screen: &Screen) -> usize {
        match screen {
            Screen::Menu => MENU.len(),
            Screen::Artists(_, v) => v.len(),
            Screen::Albums(Some(_), v) => v.len() + 1,
            Screen::Albums(None, v) => v.len(),
            Screen::Tracks(_, v) => v.len(),
            Screen::Playlists(v) => v.len(),
            Screen::Queue(v) => v.len(),
            Screen::NowPlaying => 0,
        }
    }

    /// Refresh player status; returns true if the display must change.
    pub fn poll(&mut self, be: &mut impl MusicBackend) -> bool {
        let Some(st) = self.check(be.status()) else { return true };
        let old = self.status.take();
        if old.as_ref().is_some_and(|o| o.updating && !st.updating) {
            self.refresh_stats(be); // a rescan just finished
        }
        let song_changed = old.as_ref().map(|o| (o.pos, o.queue_len)) != Some((st.pos, st.queue_len));
        if song_changed || self.current.is_none() && st.pos.is_some() {
            self.current = self.check(be.current()).flatten();
            self.need_full = true;
            // keep the queue list in sync with the playing marker
            if let Screen::Queue(_) = self.top().screen {
                if let Some(q) = self.check(be.queue()) {
                    self.top_mut().screen = Screen::Queue(q);
                }
            }
        }
        let changed = match &old {
            None => true,
            Some(o) => {
                if o.state != st.state || o.repeat != st.repeat || o.random != st.random || o.volume != st.volume {
                    self.need_full = true;
                }
                o != &st
            }
        };
        if changed && !self.need_full && self.top().screen == Screen::NowPlaying {
            self.need_progress = true;
        }
        self.status = Some(st);
        changed || self.need_full
    }

    pub fn tick(&mut self, dt_ms: u32) {
        if let Some((_, left)) = self.toast.as_mut() {
            *left = left.saturating_sub(dt_ms);
            if *left == 0 {
                self.toast = None;
                self.need_full = true;
            }
        }
        // extrapolate elapsed between polls so the bar moves smoothly
        if let Some(st) = self.status.as_mut() {
            if st.state == PlayState::Playing && st.duration > 0.0 {
                let before = st.elapsed as u32;
                st.elapsed = (st.elapsed + dt_ms as f32 / 1000.0).min(st.duration);
                if st.elapsed as u32 != before && self.top().screen == Screen::NowPlaying {
                    self.need_progress = true;
                }
            }
        }
    }

    /// Album art wanted for the current track: (uri, key), if not loaded yet.
    pub fn wanted_art(&self) -> Option<(String, String)> {
        let t = self.current.as_ref()?;
        let key = art_key(t);
        match &self.art {
            Some((k, _)) if *k == key => None,
            _ => Some((t.uri.clone(), key)),
        }
    }

    pub fn set_art(&mut self, key: String, img: Option<Image565>) {
        self.art = Some((key, img));
        if self.top().screen == Screen::NowPlaying {
            self.need_full = true;
        }
    }

    fn uris(v: &[Track]) -> Vec<String> {
        v.iter().map(|t| t.uri.clone()).collect()
    }

    pub fn handle(&mut self, ev: Event, be: &mut impl MusicBackend) -> Outcome {
        let rep = ev.state == KeyState::Repeat;
        if ev.state == KeyState::Release {
            return Outcome::Nothing;
        }
        match ev.key {
            Key::Home if !rep => return Outcome::Exit,
            Key::PlayPause if !rep => {
                let stopped = self.status.as_ref().is_none_or(|s| s.state == PlayState::Stopped);
                let r = if stopped && self.status.as_ref().is_some_and(|s| s.queue_len > 0) { be.play_pos(0) } else { be.toggle_pause() };
                self.check(r);
                self.poll(be);
                self.need_full = true;
                return Outcome::Changed;
            }
            // Volume: zpod-buttons changes it and the platform layer draws the
            // one shared overlay; the music app shows nothing of its own.
            Key::VolUp | Key::VolDown => return Outcome::Nothing,
            Key::Triangle if !rep && self.top().screen != Screen::NowPlaying => {
                self.push(Screen::NowPlaying);
                return Outcome::Changed;
            }
            _ => {}
        }
        if self.top().screen == Screen::NowPlaying {
            return self.handle_now_playing(ev, be);
        }
        let len = Self::len_of(&self.top().screen);
        let vis = self.visible_rows;
        match ev.key {
            Key::Up => {
                self.top_mut().ls.step(-1, len, vis);
                self.need_full = true;
            }
            Key::Down => {
                self.top_mut().ls.step(1, len, vis);
                self.need_full = true;
            }
            Key::Cross | Key::Left if !rep => {
                if self.stack.len() <= 1 {
                    return Outcome::Exit; // back one level = leave the app
                }
                self.pop()
            }
            Key::Ok | Key::Circle | Key::Right if !rep => self.open(be, false),
            Key::Square if !rep => self.open(be, true),
            _ => return Outcome::Nothing,
        }
        Outcome::Changed
    }

    fn handle_now_playing(&mut self, ev: Event, be: &mut impl MusicBackend) -> Outcome {
        let rep = ev.state == KeyState::Repeat;
        let r = match ev.key {
            Key::Left if !rep => be.prev(),
            Key::Right if rep => be.seek_rel(10),
            Key::Right => be.next(),
            Key::Up if !rep => {
                let m = self.status.as_ref().map(|s| s.repeat).unwrap_or_default().next();
                self.toast(m.label());
                be.set_repeat(m)
            }
            Key::Down if !rep => {
                let on = !self.status.as_ref().is_some_and(|s| s.random);
                self.toast(if on { "Shuffle on" } else { "Shuffle off" });
                be.set_random(on)
            }
            Key::Ok | Key::Circle if !rep => {
                let q = self.check(be.queue()).unwrap_or_default();
                let pos = self.status.as_ref().and_then(|s| s.pos).unwrap_or(0);
                self.push(Screen::Queue(q));
                let vis = self.visible_rows;
                let len = Self::len_of(&self.top().screen);
                self.top_mut().ls.sel = pos;
                self.top_mut().ls.scroll_into_view(len, vis);
                return Outcome::Changed;
            }
            Key::Cross if !rep => {
                self.pop();
                return Outcome::Changed;
            }
            _ => return Outcome::Nothing,
        };
        self.check(r);
        self.poll(be);
        self.need_full = true;
        Outcome::Changed
    }

    /// OK on a list row (`add` = Square: enqueue instead of play).
    fn open(&mut self, be: &mut impl MusicBackend, add: bool) {
        let sel = self.top().ls.sel;
        let screen = self.top().screen.clone();
        match screen {
            Screen::Menu if !add => match MENU[sel] {
                "Now Playing" => self.push(Screen::NowPlaying),
                m @ ("Artists" | "Albums") => {
                    if let Some(v) = self.check(be.artists()) {
                        let how = if m == "Albums" { Browse::Albums } else { Browse::Artists };
                        self.refresh_stats(be);
                        self.push(Screen::Artists(how, v));
                    }
                }
                "Playlists" => {
                    if let Some(v) = self.check(be.playlists()) {
                        self.push(Screen::Playlists(v));
                    }
                }
                "Queue" => {
                    if let Some(v) = self.check(be.queue()) {
                        self.push(Screen::Queue(v));
                    }
                }
                "Shuffle all" => {
                    if self.check(be.shuffle_all()).is_some() {
                        self.poll(be);
                        self.push(Screen::NowPlaying);
                    }
                }
                "Update library" => {
                    if self.check(be.update_library()).is_some() {
                        self.toast("Updating library...");
                    }
                }
                _ => {}
            },
            Screen::Artists(_, v) => {
                let Some(a) = v.get(sel).map(|a| &a.name) else { return };
                if add {
                    if let Some(t) = self.check(be.artist_tracks(a)) {
                        self.enqueue(be, &t);
                    }
                } else if let Some(al) = self.check(be.albums(Some(a))) {
                    self.push(Screen::Albums(Some(a.clone()), al));
                }
            }
            Screen::Albums(artist, v) => {
                let tracks = match (&artist, sel) {
                    (Some(a), 0) => self.check(be.artist_tracks(a)).map(|t| (format!("{a}: all"), t)),
                    _ => {
                        let i = if artist.is_some() { sel - 1 } else { sel };
                        let Some(al) = v.get(i) else { return };
                        self.check(be.album_tracks(al)).map(|t| (al.title_with_year(), t))
                    }
                };
                if let Some((title, t)) = tracks {
                    if add {
                        self.enqueue(be, &t);
                    } else {
                        self.push(Screen::Tracks(title, t));
                    }
                }
            }
            Screen::Playlists(v) => {
                let Some(name) = v.get(sel) else { return };
                if let Some(t) = self.check(be.playlist_tracks(name)) {
                    if add {
                        self.enqueue(be, &t);
                    } else {
                        self.push(Screen::Tracks(name.clone(), t));
                    }
                }
            }
            Screen::Tracks(_, v) => {
                if v.is_empty() {
                    return;
                }
                if add {
                    self.enqueue(be, &v[sel..sel + 1]);
                } else if self.check(be.play_tracks(&Self::uris(&v), sel)).is_some() {
                    self.poll(be);
                    self.push(Screen::NowPlaying);
                }
            }
            Screen::Queue(v) => {
                if v.is_empty() {
                    return;
                }
                if add {
                    // Square in the queue removes the entry
                    if self.check(be.delete_pos(sel)).is_some() {
                        self.toast("Removed");
                        if let Some(q) = self.check(be.queue()) {
                            let len = q.len();
                            self.top_mut().screen = Screen::Queue(q);
                            let vis = self.visible_rows;
                            self.top_mut().ls.scroll_into_view(len, vis);
                        }
                        self.poll(be);
                    }
                } else if self.check(be.play_pos(sel)).is_some() {
                    self.poll(be);
                    self.pop();
                    if self.top().screen != Screen::NowPlaying {
                        self.push(Screen::NowPlaying);
                    }
                }
            }
            _ => {}
        }
    }

    fn enqueue(&mut self, be: &mut impl MusicBackend, t: &[Track]) {
        if self.check(be.enqueue(&Self::uris(t))).is_some() {
            self.toast(if t.len() == 1 { "Added to queue".to_string() } else { format!("Queued {} tracks", t.len()) });
            self.poll(be);
        }
    }
}

#[cfg(test)]
mod tests;
