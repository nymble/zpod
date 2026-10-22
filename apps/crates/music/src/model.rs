//! Library data types, backend trait and pure helpers (tested).
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use zpod_core::PlayState;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Track {
    pub uri: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_artist: String,
    pub track: Option<u32>,
    pub disc: Option<u32>,
    pub duration: f32,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct AlbumRef {
    pub artist: String,
    pub album: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum RepeatMode {
    #[default]
    Off,
    All,
    One,
}

impl RepeatMode {
    pub fn next(self) -> Self {
        match self {
            RepeatMode::Off => RepeatMode::All,
            RepeatMode::All => RepeatMode::One,
            RepeatMode::One => RepeatMode::Off,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            RepeatMode::Off => "Repeat off",
            RepeatMode::All => "Repeat all",
            RepeatMode::One => "Repeat one",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlayerStatus {
    pub state: PlayState,
    pub repeat: RepeatMode,
    pub random: bool,
    pub elapsed: f32,
    pub duration: f32,
    pub pos: Option<usize>,
    pub queue_len: usize,
    pub volume: Option<u8>,
    pub updating: bool,
}

pub type BResult<T> = Result<T, String>;

/// Where music comes from. All calls are blocking and should be quick
/// (the MPD backend answers from its database in milliseconds).
pub trait MusicBackend {
    fn artists(&mut self) -> BResult<Vec<String>>;
    /// Albums of one artist (or all, sorted by artist then album).
    fn albums(&mut self, artist: Option<&str>) -> BResult<Vec<AlbumRef>>;
    fn album_tracks(&mut self, a: &AlbumRef) -> BResult<Vec<Track>>;
    fn artist_tracks(&mut self, artist: &str) -> BResult<Vec<Track>>;
    fn playlists(&mut self) -> BResult<Vec<String>>;
    fn playlist_tracks(&mut self, name: &str) -> BResult<Vec<Track>>;
    fn queue(&mut self) -> BResult<Vec<Track>>;
    fn status(&mut self) -> BResult<PlayerStatus>;
    fn current(&mut self) -> BResult<Option<Track>>;
    /// Replace the queue and start at `start`.
    fn play_tracks(&mut self, uris: &[String], start: usize) -> BResult<()>;
    fn enqueue(&mut self, uris: &[String]) -> BResult<()>;
    fn play_pos(&mut self, pos: usize) -> BResult<()>;
    fn delete_pos(&mut self, pos: usize) -> BResult<()>;
    fn toggle_pause(&mut self) -> BResult<()>;
    fn next(&mut self) -> BResult<()>;
    fn prev(&mut self) -> BResult<()>;
    fn seek_rel(&mut self, secs: i32) -> BResult<()>;
    fn set_repeat(&mut self, m: RepeatMode) -> BResult<()>;
    fn set_random(&mut self, on: bool) -> BResult<()>;
    /// Queue the whole library shuffled and play.
    fn shuffle_all(&mut self) -> BResult<()>;
    fn update_library(&mut self) -> BResult<()>;
}

/// Case-insensitive sort key ignoring a leading "The ".
pub fn sort_key(s: &str) -> String {
    let l = s.to_lowercase();
    match l.strip_prefix("the ") {
        Some(x) => x.to_string(),
        None => l,
    }
}

/// Sort an album's tracks: disc, track number, then uri.
pub fn sort_tracks(v: &mut [Track]) {
    v.sort_by(|a, b| {
        (a.disc.unwrap_or(1), a.track.unwrap_or(u32::MAX), &a.uri).cmp(&(b.disc.unwrap_or(1), b.track.unwrap_or(u32::MAX), &b.uri))
    });
}

/// Sort tracks of an artist: album, then disc/track.
pub fn sort_artist_tracks(v: &mut [Track]) {
    v.sort_by(|a, b| {
        (sort_key(&a.album), a.disc.unwrap_or(1), a.track.unwrap_or(u32::MAX), &a.uri).cmp(&(
            sort_key(&b.album),
            b.disc.unwrap_or(1),
            b.track.unwrap_or(u32::MAX),
            &b.uri,
        ))
    });
}

/// Parse `list album group albumartist` output pairs into album refs:
/// a group header line (`AlbumArtist: X`) applies to following `Album:` lines.
pub fn parse_grouped_albums<'a>(pairs: impl IntoIterator<Item = (&'a str, &'a str)>, group_key: &str) -> Vec<AlbumRef> {
    let mut cur = String::new();
    let mut out = Vec::new();
    for (k, v) in pairs {
        if k.eq_ignore_ascii_case(group_key) {
            cur = v.to_string();
        } else if k.eq_ignore_ascii_case("album") && !v.is_empty() {
            let artist = if cur.is_empty() { "Unknown artist".to_string() } else { cur.clone() };
            out.push(AlbumRef { artist, album: v.to_string() });
        }
    }
    sort_albums(&mut out);
    out.dedup();
    out
}

pub fn sort_albums(v: &mut [AlbumRef]) {
    v.sort_by(|a, b| (sort_key(&a.artist), sort_key(&a.album)).cmp(&(sort_key(&b.artist), sort_key(&b.album))));
}

/// Total running time of a list.
pub fn total_secs(v: &[Track]) -> f32 {
    v.iter().map(|t| t.duration).sum()
}

pub fn fmt_time(secs: f32) -> String {
    let s = if secs > 0.0 { secs as u32 } else { 0 };
    if s >= 3600 {
        alloc::format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        alloc::format!("{}:{:02}", s / 60, s % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn t(uri: &str, album: &str, disc: Option<u32>, track: Option<u32>) -> Track {
        Track { uri: uri.into(), album: album.into(), disc, track, ..Default::default() }
    }

    #[test]
    fn grouped_album_parsing() {
        let pairs = vec![
            ("AlbumArtist", "The Zombies"),
            ("Album", "Odessey and Oracle"),
            ("AlbumArtist", "ABBA"),
            ("Album", "Arrival"),
            ("Album", "Abbey"),
            ("AlbumArtist", ""),
            ("Album", "Loose Tracks"),
            ("Album", ""),
        ];
        let v = parse_grouped_albums(pairs, "albumartist");
        let names: Vec<_> = v.iter().map(|a| (a.artist.as_str(), a.album.as_str())).collect();
        assert_eq!(names, [("ABBA", "Abbey"), ("ABBA", "Arrival"), ("Unknown artist", "Loose Tracks"), ("The Zombies", "Odessey and Oracle")]);
    }

    #[test]
    fn track_ordering() {
        let mut v = vec![t("c", "A", Some(2), Some(1)), t("b", "A", None, Some(2)), t("a", "A", Some(1), Some(1)), t("z", "A", None, None)];
        sort_tracks(&mut v);
        let o: Vec<_> = v.iter().map(|t| t.uri.as_str()).collect();
        assert_eq!(o, ["a", "b", "z", "c"]);
        let mut w = vec![t("2", "Beta", None, Some(1)), t("1", "alpha", None, Some(2)), t("0", "alpha", None, Some(1))];
        sort_artist_tracks(&mut w);
        let o: Vec<_> = w.iter().map(|t| t.uri.as_str()).collect();
        assert_eq!(o, ["0", "1", "2"]);
    }

    #[test]
    fn misc() {
        assert_eq!(sort_key("The Who"), "who");
        assert_eq!(fmt_time(3599.9), "59:59");
        assert_eq!(fmt_time(-3.0), "0:00");
        assert_eq!(RepeatMode::Off.next().next().next(), RepeatMode::Off);
    }
}
