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
        Ok(vec!["Road trip".into()])
    }
    fn playlist_tracks(&mut self, _n: &str) -> BResult<Vec<Track>> {
        Ok(self.lib[..2].to_vec())
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
    assert!(matches!(app.top().screen, Screen::Tracks(ref n, ref v) if n == "Road trip" && v.len() == 2));
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
