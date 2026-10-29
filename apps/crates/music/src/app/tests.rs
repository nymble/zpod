extern crate std;
use super::*;
use alloc::string::ToString;

/// In-memory library + player standing in for MPD.
#[derive(Default)]
struct Fake {
    lib: Vec<Track>,
    queue: Vec<Track>,
    pos: Option<usize>,
    state: PlayState,
    repeat: RepeatMode,
    random: bool,
    fail: bool,
    pls: Vec<(String, Vec<String>)>,
}

fn tr(artist: &str, album: &str, n: u32, title: &str) -> Track {
    Track {
        uri: format!("{artist}/{album}/{n:02} {title}.flac"),
        title: title.into(),
        artist: artist.into(),
        album_artist: artist.into(),
        album: album.into(),
        track: Some(n),
        disc: None,
        duration: 180.0 + n as f32,
        year: match album {
            "First" => Some(1999),
            "Second" => Some(1995),
            _ => None,
        },
    }
}

impl Fake {
    fn new() -> Self {
        Fake {
            lib: vec![
                tr("Alpha", "First", 2, "Two"),
                tr("Alpha", "First", 1, "One"),
                tr("Alpha", "Second", 1, "Uno"),
                tr("Beta", "Gamma", 1, "G1"),
            ],
            state: PlayState::Stopped,
            pls: vec![("Road trip".into(), vec!["Alpha/First/02 Two.flac".into(), "Alpha/First/01 One.flac".into()])],
            ..Default::default()
        }
    }
    fn err<T>(&self) -> BResult<T> {
        Err("MPD not running".to_string())
    }
}

impl MusicBackend for Fake {
    fn artists(&mut self) -> BResult<Vec<ArtistRef>> {
        if self.fail {
            return self.err();
        }
        Ok(artists_from_albums(&self.albums(None)?))
    }
    fn albums(&mut self, artist: Option<&str>) -> BResult<Vec<AlbumRef>> {
        let mut v: Vec<AlbumRef> = Vec::new();
        for t in self.lib.iter().filter(|t| artist.is_none_or(|a| a == t.album_artist)) {
            match v.iter_mut().find(|a| a.artist == t.album_artist && a.album == t.album) {
                Some(a) => {
                    a.tracks += 1;
                    a.secs += t.duration as u32;
                }
                None => v.push(AlbumRef { year: t.year, tracks: 1, secs: t.duration as u32, ..AlbumRef::new(t.album_artist.clone(), t.album.clone()) }),
            }
        }
        if artist.is_some() { sort_albums_by_year(&mut v) } else { sort_albums(&mut v) }
        Ok(v)
    }
    fn stats(&mut self) -> BResult<LibraryStats> {
        let al = self.albums(None)?;
        Ok(LibraryStats { artists: artists_from_albums(&al).len() as u32, albums: al.len() as u32, songs: self.lib.len() as u32 })
    }
    fn album_tracks(&mut self, a: &AlbumRef) -> BResult<Vec<Track>> {
        let mut v: Vec<Track> = self.lib.iter().filter(|t| t.album == a.album && t.album_artist == a.artist).cloned().collect();
        sort_tracks(&mut v);
        Ok(v)
    }
    fn artist_tracks(&mut self, artist: &str) -> BResult<Vec<Track>> {
        let mut v: Vec<Track> = self.lib.iter().filter(|t| t.album_artist == artist).cloned().collect();
        sort_artist_tracks(&mut v);
        Ok(v)
    }
    fn playlists(&mut self) -> BResult<Vec<String>> {
        Ok(self.pls.iter().map(|(n, _)| n.clone()).collect())
    }
    fn playlist_tracks(&mut self, n: &str) -> BResult<Vec<Track>> {
        let Some((_, u)) = self.pls.iter().find(|(x, _)| x == n) else { return Err("No such playlist".into()) };
        Ok(u.iter().map(|u| self.lib.iter().find(|t| &t.uri == u).unwrap().clone()).collect())
    }
    fn playlist_add(&mut self, name: &str, uris: &[String]) -> BResult<()> {
        match self.pls.iter_mut().find(|(x, _)| x == name) {
            Some((_, v)) => v.extend(uris.iter().cloned()),
            None => self.pls.push((name.into(), uris.to_vec())),
        }
        Ok(())
    }
    fn playlist_delete(&mut self, name: &str) -> BResult<()> {
        self.pls.retain(|(x, _)| x != name);
        Ok(())
    }
    fn queue(&mut self) -> BResult<Vec<Track>> {
        Ok(self.queue.clone())
    }
    fn status(&mut self) -> BResult<PlayerStatus> {
        if self.fail {
            return self.err();
        }
        Ok(PlayerStatus {
            state: self.state,
            repeat: self.repeat,
            random: self.random,
            elapsed: 0.0,
            duration: self.pos.map(|p| self.queue[p].duration).unwrap_or(0.0),
            pos: self.pos,
            queue_len: self.queue.len(),
            volume: Some(50),
            updating: false,
        })
    }
    fn current(&mut self) -> BResult<Option<Track>> {
        Ok(self.pos.map(|p| self.queue[p].clone()))
    }
    fn play_tracks(&mut self, uris: &[String], start: usize) -> BResult<()> {
        self.queue = uris.iter().map(|u| self.lib.iter().find(|t| &t.uri == u).unwrap().clone()).collect();
        self.pos = Some(start);
        self.state = PlayState::Playing;
        Ok(())
    }
    fn enqueue(&mut self, uris: &[String]) -> BResult<()> {
        for u in uris {
            let t = self.lib.iter().find(|t| &t.uri == u).unwrap().clone();
            self.queue.push(t);
        }
        Ok(())
    }
    fn play_pos(&mut self, pos: usize) -> BResult<()> {
        self.pos = Some(pos);
        self.state = PlayState::Playing;
        Ok(())
    }
    fn delete_pos(&mut self, pos: usize) -> BResult<()> {
        self.queue.remove(pos);
        if self.queue.is_empty() {
            self.pos = None;
            self.state = PlayState::Stopped;
        } else if let Some(p) = self.pos {
            self.pos = Some(p.min(self.queue.len() - 1));
        }
        Ok(())
    }
    fn toggle_pause(&mut self) -> BResult<()> {
        self.state = if self.state == PlayState::Playing { PlayState::Paused } else { PlayState::Playing };
        Ok(())
    }
    fn next(&mut self) -> BResult<()> {
        if let Some(p) = self.pos {
            self.pos = Some((p + 1).min(self.queue.len() - 1));
        }
        Ok(())
    }
    fn prev(&mut self) -> BResult<()> {
        if let Some(p) = self.pos {
            self.pos = Some(p.saturating_sub(1));
        }
        Ok(())
    }
    fn seek_rel(&mut self, _s: i32) -> BResult<()> {
        Ok(())
    }
    fn set_repeat(&mut self, m: RepeatMode) -> BResult<()> {
        self.repeat = m;
        Ok(())
    }
    fn set_random(&mut self, on: bool) -> BResult<()> {
        self.random = on;
        Ok(())
    }
    fn shuffle_all(&mut self) -> BResult<()> {
        let u: Vec<String> = self.lib.iter().map(|t| t.uri.clone()).collect();
        self.play_tracks(&u, 0)?;
        self.random = true;
        Ok(())
    }
    fn update_library(&mut self) -> BResult<()> {
        Ok(())
    }
}

fn press(k: Key) -> Event {
    Event::press(k)
}

#[test]
fn browse_artist_album_and_play() {
    let mut be = Fake::new();
    let mut app = MusicApp::new();
    app.start(&mut be);
    assert_eq!(app.top().screen, Screen::Menu);
    app.handle(press(Key::Down), &mut be); // Artists
    app.handle(press(Key::Ok), &mut be);
    assert_eq!(app.top().screen, Screen::Artists(Browse::Artists, vec![ArtistRef::new("Alpha", 2), ArtistRef::new("Beta", 1)]));
    assert_eq!(crate::view::title(&app), "Artists (2)");
    app.handle(press(Key::Right), &mut be);
    match &app.top().screen {
        Screen::Albums(Some(a), v) => {
            assert_eq!(a, "Alpha");
            // release order: Second (1995) before First (1999), with counts
            assert_eq!(v.iter().map(|a| (a.album.as_str(), a.year, a.tracks)).collect::<Vec<_>>(), [("Second", Some(1995), 1), ("First", Some(1999), 2)]);
        }
        s => panic!("{s:?}"),
    }
    assert_eq!(crate::view::title(&app), "Alpha (2)");
    assert_eq!(MusicApp::len_of(&app.top().screen), 3); // + "All tracks"
    app.handle(press(Key::Down), &mut be);
    app.handle(press(Key::Down), &mut be); // "First"
    app.handle(press(Key::Circle), &mut be);
    assert_eq!(crate::view::title(&app), "First (1999) \u{b7} 2");
    match &app.top().screen {
        Screen::Tracks(t, v) => {
            assert_eq!(t, "First (1999)");
            assert_eq!(v.iter().map(|t| t.title.as_str()).collect::<Vec<_>>(), ["One", "Two"]);
        }
        s => panic!("{s:?}"),
    }
    app.handle(press(Key::Down), &mut be); // "Two"
    app.handle(press(Key::Ok), &mut be);
    assert_eq!(app.top().screen, Screen::NowPlaying);
    assert_eq!(app.current.as_ref().unwrap().title, "Two");
    assert_eq!(be.queue.len(), 2);
    assert_eq!(app.wanted_art().unwrap().1, "Alpha\u{1f}First");
    // Left = previous track
    app.handle(press(Key::Left), &mut be);
    assert_eq!(app.current.as_ref().unwrap().title, "One");
    // Up cycles repeat, Down toggles shuffle
    app.handle(press(Key::Up), &mut be);
    app.handle(press(Key::Up), &mut be);
    assert_eq!(be.repeat, RepeatMode::One);
    app.handle(press(Key::Down), &mut be);
    assert!(be.random);
    // Play/Pause
    app.handle(press(Key::PlayPause), &mut be);
    assert_eq!(be.state, PlayState::Paused);
    // X back to the track list, Home exits
    app.handle(press(Key::Cross), &mut be);
    assert!(matches!(app.top().screen, Screen::Tracks(..)));
    assert_eq!(app.handle(press(Key::Home), &mut be), Outcome::Exit);
}

#[test]
fn queue_add_play_and_remove() {
    let mut be = Fake::new();
    let mut app = MusicApp::new();
    app.start(&mut be);
    // Albums -> artists (Alpha, Beta) -> Beta -> Square on "Gamma"
    app.handle(press(Key::Down), &mut be);
    app.handle(press(Key::Down), &mut be);
    app.handle(press(Key::Ok), &mut be);
    assert!(matches!(app.top().screen, Screen::Artists(Browse::Albums, ref v) if v.len() == 2));
    assert_eq!(crate::view::title(&app), "Albums (3) by artist");
    app.handle(press(Key::Up), &mut be); // wraps to last = Beta
    app.handle(press(Key::Ok), &mut be);
    assert!(matches!(app.top().screen, Screen::Albums(Some(ref a), _) if a == "Beta"));
    app.handle(press(Key::Up), &mut be); // wraps to last = Gamma
    app.handle(press(Key::Square), &mut be);
    assert_eq!(be.queue.len(), 1);
    assert_eq!(app.toast.as_ref().unwrap().0, "Added to queue");
    // Triangle -> now playing; OK -> queue; OK plays the selected entry
    app.handle(press(Key::Triangle), &mut be);
    assert_eq!(app.top().screen, Screen::NowPlaying);
    app.handle(press(Key::Ok), &mut be);
    assert!(matches!(app.top().screen, Screen::Queue(ref q) if q.len() == 1));
    app.handle(press(Key::Ok), &mut be);
    assert_eq!(app.top().screen, Screen::NowPlaying);
    assert_eq!(be.state, PlayState::Playing);
    // queue -> Square removes
    app.handle(press(Key::Ok), &mut be);
    app.handle(press(Key::Square), &mut be);
    assert!(be.queue.is_empty());
    assert!(matches!(app.top().screen, Screen::Queue(ref q) if q.is_empty()));
}

#[test]
fn shuffle_all_and_playlists() {
    let mut be = Fake::new();
    let mut app = MusicApp::new();
    app.start(&mut be);
    for _ in 0..5 {
        app.handle(press(Key::Down), &mut be);
    }
    app.handle(press(Key::Ok), &mut be); // Shuffle all
    assert!(be.random && be.queue.len() == 4);
    assert_eq!(app.top().screen, Screen::NowPlaying);
    app.handle(press(Key::Cross), &mut be);
    for _ in 0..2 {
        app.handle(press(Key::Up), &mut be);
    }
    app.handle(press(Key::Ok), &mut be); // Playlists
    assert_eq!(app.top().screen, Screen::Playlists(vec!["Road trip".into()]));
    app.handle(press(Key::Ok), &mut be);
    assert_eq!(app.top().screen, Screen::PlaylistMenu("Road trip".into()));
    app.handle(press(Key::Down), &mut be);
    app.handle(press(Key::Down), &mut be);
    app.handle(press(Key::Ok), &mut be); // Tracks
    assert!(matches!(app.top().screen, Screen::Tracks(ref n, ref v) if n == "Road trip" && v.len() == 2));
}

fn rep_ev(k: Key) -> Event {
    Event { key: k, state: KeyState::Repeat }
}

fn big_library() -> Fake {
    let mut be = Fake::new();
    be.lib.clear();
    for (i, a) in ["Abba", "Air", "The Beatles", "Bjork", "Blur", "Can", "Cream", "Devo", "Eno", "Faust", "Feist", "Gong"].iter().enumerate() {
        be.lib.push(tr(a, "LP", 1, &format!("t{i}")));
    }
    be
}

#[test]
fn hold_up_down_jumps_by_letter_in_artists() {
    let mut be = big_library();
    let mut app = MusicApp::new();
    app.start(&mut be);
    app.handle(press(Key::Down), &mut be);
    app.handle(press(Key::Ok), &mut be); // Artists
    let name = |app: &MusicApp| match &app.top().screen {
        Screen::Artists(_, v) => v[app.top().ls.sel].name.clone(),
        _ => panic!("not artists"),
    };
    assert_eq!(name(&app), "Abba");
    // a held key: first press, then repeats; single steps until FAST_AFTER
    app.handle(press(Key::Down), &mut be);
    for _ in 0..FAST_AFTER {
        app.handle(rep_ev(Key::Down), &mut be);
    }
    assert_eq!(app.top().ls.sel, 1 + FAST_AFTER as usize);
    assert!(app.jump.is_none());
    let before = index_letter(&name(&app));
    app.handle(rep_ev(Key::Down), &mut be); // now a letter per step
    let after = index_letter(&name(&app));
    assert_ne!(before, after);
    assert_eq!(app.jump.map(|j| j.0), Some(after));
    // the first artist of that letter
    assert!(matches!(after, 'D' | 'E' | 'F' | 'G' | 'A'));
    // release + new press = single step again; the letter disappears after a while
    app.handle(press(Key::Up), &mut be);
    app.tick(JUMP_SHOW_MS + 1);
    assert!(app.jump.is_none());
    // held Up jumps back a letter group at a time ("The Beatles" sorts under B)
    let mut app = MusicApp::new();
    app.start(&mut be);
    app.handle(press(Key::Down), &mut be);
    app.handle(press(Key::Ok), &mut be);
    app.handle(press(Key::Up), &mut be); // wraps to Gong
    for _ in 0..=FAST_AFTER {
        app.handle(rep_ev(Key::Up), &mut be);
    }
    assert!(app.jump.is_some());
}

#[test]
fn hold_scrolls_by_page_in_track_lists() {
    let mut be = Fake::new();
    for n in 3..40 {
        be.lib.push(tr("Alpha", "First", n, &format!("n{n}")));
    }
    let mut app = MusicApp::new();
    app.start(&mut be);
    app.handle(press(Key::Down), &mut be);
    app.handle(press(Key::Ok), &mut be); // Artists
    app.handle(press(Key::Ok), &mut be); // Alpha
    app.handle(press(Key::Down), &mut be); // Second (1995)
    app.handle(press(Key::Down), &mut be); // First (1999)
    app.handle(press(Key::Ok), &mut be);
    assert!(matches!(app.top().screen, Screen::Tracks(_, ref v) if v.len() == 39));
    app.visible_rows = 5;
    app.handle(press(Key::Down), &mut be);
    for _ in 0..FAST_AFTER {
        app.handle(rep_ev(Key::Down), &mut be);
    }
    let s = app.top().ls.sel;
    app.handle(rep_ev(Key::Down), &mut be);
    assert_eq!(app.top().ls.sel, s + 5);
    assert!(app.jump.is_none());
}

#[test]
fn playlists_add_track_and_album_play_and_delete() {
    let mut be = Fake::new();
    let u: Vec<String> = be.lib.iter().map(|t| t.uri.clone()).collect();
    be.play_tracks(&u, 2).unwrap(); // Alpha / Second / Uno
    let mut app = MusicApp::new();
    app.start(&mut be);
    assert_eq!(app.top().screen, Screen::NowPlaying);
    // Square: add the track -> picker (New playlist, Road trip)
    app.handle(press(Key::Square), &mut be);
    assert!(matches!(app.top().screen, Screen::AddTo(ref w, ref u, ref n) if w == "Track" && u.len() == 1 && n.len() == 1));
    assert_eq!(crate::view::title(&app), "Add track to playlist");
    app.handle(press(Key::Ok), &mut be); // New playlist -> "Playlist 1"
    assert_eq!(app.top().screen, Screen::NowPlaying);
    assert_eq!(be.pls[1], ("Playlist 1".to_string(), vec!["Alpha/Second/01 Uno.flac".to_string()]));
    assert!(app.toast.as_ref().unwrap().0.contains("Playlist 1"));
    // Triangle: add the whole album to the existing "Road trip"
    be.play_tracks(&u, 0).unwrap(); // Alpha / First / Two
    app.poll(&mut be);
    app.handle(press(Key::Triangle), &mut be);
    assert!(matches!(app.top().screen, Screen::AddTo(ref w, ref u, _) if w.starts_with("Album") && u.len() == 2));
    app.handle(press(Key::Down), &mut be); // Road trip (names in MPD order)
    app.handle(press(Key::Ok), &mut be);
    assert_eq!(be.pls[0].1.len(), 4);
    // X cancels the picker without adding
    app.handle(press(Key::Square), &mut be);
    app.handle(press(Key::Cross), &mut be);
    assert_eq!(app.top().screen, Screen::NowPlaying);
    assert_eq!(be.pls.len(), 2);
    // Playlists > Playlist 1 > Play
    app.handle(press(Key::Cross), &mut be);
    while app.top().screen != Screen::Menu {
        app.handle(press(Key::Cross), &mut be);
    }
    app.top_mut_for_tests().ls.sel = 3;
    app.handle(press(Key::Ok), &mut be);
    assert_eq!(app.top().screen, Screen::Playlists(vec!["Road trip".into(), "Playlist 1".into()]));
    app.handle(press(Key::Down), &mut be);
    app.handle(press(Key::Ok), &mut be);
    assert_eq!(app.top().screen, Screen::PlaylistMenu("Playlist 1".into()));
    app.handle(press(Key::Ok), &mut be); // Play
    assert_eq!(app.top().screen, Screen::NowPlaying);
    assert_eq!(be.queue.len(), 1);
    assert_eq!(be.state, PlayState::Playing);
    // back to the playlist menu, Delete playlist, X cancels, then OK deletes
    app.handle(press(Key::Cross), &mut be);
    app.top_mut_for_tests().ls.sel = 3;
    app.handle(press(Key::Ok), &mut be);
    assert_eq!(app.top().screen, Screen::ConfirmDelete("Playlist 1".into()));
    app.handle(press(Key::Left), &mut be);
    assert_eq!(app.top().screen, Screen::PlaylistMenu("Playlist 1".into()));
    assert_eq!(be.pls.len(), 2);
    app.handle(press(Key::Ok), &mut be);
    app.handle(press(Key::Ok), &mut be);
    assert_eq!(be.pls.len(), 1);
    assert_eq!(app.top().screen, Screen::Playlists(vec!["Road trip".into()]));
    // Shuffle play turns shuffle on
    app.handle(press(Key::Ok), &mut be);
    app.handle(press(Key::Down), &mut be);
    app.handle(press(Key::Ok), &mut be);
    assert!(be.random && be.queue.len() == 4);
}

#[test]
fn add_with_nothing_playing_says_so() {
    let mut be = Fake::new();
    let mut app = MusicApp::new();
    app.start(&mut be);
    app.handle(press(Key::Triangle), &mut be);
    assert_eq!(app.top().screen, Screen::NowPlaying);
    app.handle(press(Key::Square), &mut be);
    assert_eq!(app.top().screen, Screen::NowPlaying);
    assert_eq!(app.toast.as_ref().unwrap().0, "Nothing playing");
}

#[test]
fn menu_shows_library_counts() {
    let mut be = Fake::new();
    let mut app = MusicApp::new();
    app.start(&mut be);
    assert_eq!(app.stats, Some(LibraryStats { artists: 2, albums: 3, songs: 4 }));
}

#[test]
fn backend_errors_are_shown_not_fatal() {
    let mut be = Fake::new();
    be.fail = true;
    let mut app = MusicApp::new();
    app.start(&mut be);
    assert_eq!(app.error.as_deref(), Some("MPD not running"));
    app.handle(press(Key::Down), &mut be);
    app.handle(press(Key::Ok), &mut be);
    assert_eq!(app.top().screen, Screen::Menu);
    be.fail = false;
    app.poll(&mut be);
    assert!(app.error.is_none());
}

#[test]
fn starts_on_now_playing_when_music_is_on() {
    let mut be = Fake::new();
    let u: Vec<String> = be.lib.iter().map(|t| t.uri.clone()).collect();
    be.play_tracks(&u, 1).unwrap();
    let mut app = MusicApp::new();
    app.start(&mut be);
    assert_eq!(app.top().screen, Screen::NowPlaying);
    assert_eq!(app.stack.len(), 2);
}
