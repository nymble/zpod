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
        year: s.date.as_deref().and_then(parse_year),
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
    fn artists(&mut self) -> BResult<Vec<ArtistRef>> {
        self.with(|c| {
            let p = c.cmd("list album group albumartist")?;
            let v = artists_from_albums(&parse_grouped_albums(p.iter().map(|(k, v)| (k.as_str(), v.as_str())), "albumartist"));
            if !v.is_empty() {
                return Ok(v);
            }
            // no Album tags at all: plain artist names, counts unknown
            Ok(c.list("artist", None)?.into_iter().map(|a| ArtistRef::new(a, 0)).collect())
        })
    }
    fn albums(&mut self, artist: Option<&str>) -> BResult<Vec<AlbumRef>> {
        self.with(|c| match artist {
            Some(a) => {
                let f = quote(&filter_eq("albumartist", a));
                let dated = c.cmd(&format!("list album {f} group date"))?;
                let mut v = parse_dated_albums(dated.iter().map(|(k, v)| (k.as_str(), v.as_str())), a);
                let counts = c.cmd(&format!("count {f} group album"))?;
                apply_album_counts(&mut v, counts.iter().map(|(k, v)| (k.as_str(), v.as_str())));
                sort_albums_by_year(&mut v);
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
    fn playlist_add(&mut self, name: &str, uris: &[String]) -> BResult<()> {
        let mut batch = String::from("command_list_begin\n");
        for u in uris {
            batch.push_str(&format!("playlistadd {} {}\n", quote(name), quote(u)));
        }
        batch.push_str("command_list_end");
        self.with(|c| c.simple(&batch))
    }
    fn playlist_delete(&mut self, name: &str) -> BResult<()> {
        self.with(|c| c.simple(&format!("rm {}", quote(name))))
    }
    fn play_playlist(&mut self, name: &str, shuffle: bool) -> BResult<()> {
        let sh = if shuffle { "shuffle\n" } else { "" };
        self.with(|c| c.simple(&format!("command_list_begin\nclear\nload {}\n{sh}play 0\ncommand_list_end", quote(name))))
    }
    fn stats(&mut self) -> BResult<LibraryStats> {
        let p = self.with(|c| c.cmd("stats"))?;
        let get = |key: &str| p.iter().find(|(k, _)| k == key).and_then(|(_, v)| v.trim().parse().ok()).unwrap_or(0);
        Ok(LibraryStats { artists: get("artists"), albums: get("albums"), songs: get("songs") })
    }
}
