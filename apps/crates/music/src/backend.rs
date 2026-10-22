//! MPD implementation of `music_core::MusicBackend`.
use music_core::model::*;
use zpod_core::PlayState;
use zpod_mpd::{filter_eq, quote, Client, Song};

pub struct Mpd {
    c: Option<Client>,
}

fn track(s: Song) -> Track {
    Track {
        title: s.display_title(),
        artist: s.display_artist().to_string(),
        album_artist: s.group_artist().to_string(),
        album: s.album.clone().unwrap_or_default(),
        track: s.track,
        disc: s.disc,
        duration: s.duration,
        uri: s.file,
    }
}

impl Mpd {
    pub fn new() -> Self {
        Mpd { c: None }
    }

    fn with<T>(&mut self, f: impl FnOnce(&mut Client) -> zpod_mpd::Result<T>) -> BResult<T> {
        if self.c.is_none() {
            self.c = Some(Client::connect_default().map_err(|e| e.to_string())?);
        }
        let r = f(self.c.as_mut().unwrap());
        match r {
            Ok(v) => Ok(v),
            Err(zpod_mpd::Error::Ack(m)) => Err(m),
            Err(e) => {
                self.c = None; // reconnect next time
                Err(e.to_string())
            }
        }
    }
}

impl MusicBackend for Mpd {
    fn artists(&mut self) -> BResult<Vec<String>> {
        self.with(|c| {
            let v = c.list("albumartist", None)?;
            if v.is_empty() { c.list("artist", None) } else { Ok(v) }
        })
    }
    fn albums(&mut self, artist: Option<&str>) -> BResult<Vec<AlbumRef>> {
        self.with(|c| match artist {
            Some(a) => {
                let mut v: Vec<AlbumRef> = c
                    .list("album", Some(&filter_eq("albumartist", a)))?
                    .into_iter()
                    .map(|al| AlbumRef { artist: a.to_string(), album: al })
                    .collect();
                sort_albums(&mut v);
                Ok(v)
            }
            None => {
                let p = c.cmd("list album group albumartist")?;
                Ok(parse_grouped_albums(p.iter().map(|(k, v)| (k.as_str(), v.as_str())), "albumartist"))
            }
        })
    }
    fn album_tracks(&mut self, a: &AlbumRef) -> BResult<Vec<Track>> {
        let f = format!("({} AND {})", filter_eq("albumartist", &a.artist), filter_eq("album", &a.album));
        let mut v: Vec<Track> = self.with(|c| c.find(&f))?.into_iter().map(track).collect();
        sort_tracks(&mut v);
        Ok(v)
    }
    fn artist_tracks(&mut self, artist: &str) -> BResult<Vec<Track>> {
        let f = filter_eq("albumartist", artist);
        let mut v: Vec<Track> = self.with(|c| c.find(&f))?.into_iter().map(track).collect();
        sort_artist_tracks(&mut v);
        Ok(v)
    }
    fn playlists(&mut self) -> BResult<Vec<String>> {
        self.with(|c| c.playlists())
    }
    fn playlist_tracks(&mut self, name: &str) -> BResult<Vec<Track>> {
        Ok(self.with(|c| c.playlist_songs(name))?.into_iter().map(track).collect())
    }
    fn queue(&mut self) -> BResult<Vec<Track>> {
        Ok(self.with(|c| c.queue())?.into_iter().map(track).collect())
    }
    fn status(&mut self) -> BResult<PlayerStatus> {
        let s = self.with(|c| c.status())?;
        Ok(PlayerStatus {
            state: match s.state.as_str() {
                "play" => PlayState::Playing,
                "pause" => PlayState::Paused,
                _ => PlayState::Stopped,
            },
            repeat: match (s.repeat, s.single) {
                (true, true) => RepeatMode::One,
                (true, false) => RepeatMode::All,
                _ => RepeatMode::Off,
            },
            random: s.random,
            elapsed: s.elapsed,
            duration: s.duration,
            pos: s.song,
            queue_len: s.playlist_len,
            volume: s.volume,
            updating: s.updating_db,
        })
    }
    fn current(&mut self) -> BResult<Option<Track>> {
        Ok(self.with(|c| c.current_song())?.map(track))
    }
    fn play_tracks(&mut self, uris: &[String], start: usize) -> BResult<()> {
        self.with(|c| c.play_uris(uris, start))
    }
    fn enqueue(&mut self, uris: &[String]) -> BResult<()> {
        self.with(|c| c.add_uris(uris))
    }
    fn play_pos(&mut self, pos: usize) -> BResult<()> {
        self.with(|c| c.simple(&format!("play {pos}")))
    }
    fn delete_pos(&mut self, pos: usize) -> BResult<()> {
        self.with(|c| c.simple(&format!("delete {pos}")))
    }
    fn toggle_pause(&mut self) -> BResult<()> {
        self.with(|c| {
            let s = c.status()?;
            match s.state.as_str() {
                "play" => c.simple("pause 1"),
                "pause" => c.simple("pause 0"),
                _ => c.simple("play"),
            }
        })
    }
    fn next(&mut self) -> BResult<()> {
        self.with(|c| c.simple("next"))
    }
    fn prev(&mut self) -> BResult<()> {
        self.with(|c| c.simple("previous"))
    }
    fn seek_rel(&mut self, secs: i32) -> BResult<()> {
        self.with(|c| c.simple(&format!("seekcur {}", quote(&format!("{secs:+}")))))
    }
    fn set_repeat(&mut self, m: RepeatMode) -> BResult<()> {
        let (r, s) = match m {
            RepeatMode::Off => (0, 0),
            RepeatMode::All => (1, 0),
            RepeatMode::One => (1, 1),
        };
        self.with(|c| c.simple(&format!("command_list_begin\nrepeat {r}\nsingle {s}\ncommand_list_end")))
    }
    fn set_random(&mut self, on: bool) -> BResult<()> {
        self.with(|c| c.simple(&format!("random {}", on as u8)))
    }
    fn shuffle_all(&mut self) -> BResult<()> {
        self.with(|c| c.simple("command_list_begin\nclear\nadd \"/\"\nshuffle\nplay 0\ncommand_list_end"))
    }
    fn update_library(&mut self) -> BResult<()> {
        self.with(|c| c.simple("update"))
    }
}
