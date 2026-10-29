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
    /// Release year from the Date tag (first 4-digit year).
    pub year: Option<u16>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct AlbumRef {
    pub artist: String,
    pub album: String,
    /// Earliest year among the album's tracks, if tagged.
    pub year: Option<u16>,
    /// Number of tracks and total seconds (0 when unknown).
    pub tracks: u32,
    pub secs: u32,
}

impl AlbumRef {
    pub fn new(artist: impl Into<String>, album: impl Into<String>) -> Self {
        AlbumRef { artist: artist.into(), album: album.into(), ..Default::default() }
    }
    /// "Milestones (1958)" or just "Milestones".
    pub fn title_with_year(&self) -> String {
        with_year(&self.album, self.year)
    }
    /// "6 tracks \u{b7} 38:12", or "" when counts are unknown.
    pub fn summary(&self) -> String {
        match (self.tracks, self.secs) {
            (0, _) => String::new(),
            (n, 0) => plural(n, "track"),
            (n, s) => alloc::format!("{} \u{b7} {}", plural(n, "track"), fmt_time(s as f32)),
        }
    }
}

/// An album artist and how many albums they have.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ArtistRef {
    pub name: String,
    pub albums: u32,
}

impl ArtistRef {
    pub fn new(name: impl Into<String>, albums: u32) -> Self {
        ArtistRef { name: name.into(), albums }
    }
}

/// Library totals (MPD `stats`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LibraryStats {
    pub artists: u32,
    pub albums: u32,
    pub songs: u32,
}

/// "1 album", "19 albums".
pub fn plural(n: u32, word: &str) -> String {
    if n == 1 { alloc::format!("1 {word}") } else { alloc::format!("{n} {word}s") }
}

/// "Title (1958)" when a year is known.
pub fn with_year(title: &str, year: Option<u16>) -> String {
    match year {
        Some(y) => alloc::format!("{title} ({y})"),
        None => title.to_string(),
    }
}

/// First plausible 4-digit year in a Date tag ("1958", "1958-03-04", "03/1958").
pub fn parse_year(date: &str) -> Option<u16> {
    let b = date.as_bytes();
    let mut i = 0;
    while i + 4 <= b.len() {
        if b[i..i + 4].iter().all(u8::is_ascii_digit) && (i + 4 == b.len() || !b[i + 4].is_ascii_digit()) {
            let y: u16 = core::str::from_utf8(&b[i..i + 4]).ok()?.parse().ok()?;
            if (1000..=2999).contains(&y) {
                return Some(y);
            }
        }
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        i += 1;
    }
    None
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
    /// Album artists with their album counts, sorted by name.
    fn artists(&mut self) -> BResult<Vec<ArtistRef>>;
    /// Albums of one artist with year and track counts, oldest first
    /// (or all albums, sorted by artist then album; counts may be 0).
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
    /// Append tracks to a stored playlist (created if missing).
    fn playlist_add(&mut self, _name: &str, _uris: &[String]) -> BResult<()> {
        Err("playlists are not supported by this backend".to_string())
    }
    /// Delete a stored playlist.
    fn playlist_delete(&mut self, _name: &str) -> BResult<()> {
        Err("playlists are not supported by this backend".to_string())
    }
    /// Replace the queue with a playlist and play it (shuffled order if asked).
    fn play_playlist(&mut self, name: &str, shuffle: bool) -> BResult<()> {
        let t = self.playlist_tracks(name)?;
        let u: Vec<String> = t.iter().map(|t| t.uri.clone()).collect();
        if u.is_empty() {
            return Err("playlist is empty".to_string());
        }
        self.play_tracks(&u, 0)?;
        if shuffle {
            self.set_random(true)?;
        }
        Ok(())
    }
    /// Library totals for the menu; backends without them return zeros.
    fn stats(&mut self) -> BResult<LibraryStats> {
        Ok(LibraryStats::default())
    }
}

/// Case-insensitive sort key ignoring a leading "The ".
pub fn sort_key(s: &str) -> String {
    let l = s.to_lowercase();
    match l.strip_prefix("the ") {
        Some(x) => x.to_string(),
        None => l,
    }
}

/// Fast-scroll index letter: the first letter of [`sort_key`] in upper case
/// ("The Who" -> 'W'), or '#' for digits and symbols.
pub fn index_letter(s: &str) -> char {
    match sort_key(s).chars().next() {
        Some(c) if c.is_alphabetic() => c.to_uppercase().next().unwrap_or(c),
        _ => '#',
    }
}

/// Jump-by-letter: from `sel`, move to the first row of the next letter
/// group (`dir` > 0) or to the start of the current/previous group
/// (`dir` < 0). Wraps at the ends. `keys` holds one index letter per row.
pub fn letter_jump(keys: &[char], sel: usize, dir: i32) -> usize {
    let n = keys.len();
    if n == 0 {
        return 0;
    }
    let sel = sel.min(n - 1);
    let start_of = |i: usize| {
        let mut j = i;
        while j > 0 && keys[j - 1] == keys[i] {
            j -= 1;
        }
        j
    };
    if dir > 0 {
        (sel + 1..n).find(|&i| keys[i] != keys[sel]).unwrap_or(0)
    } else {
        let s = start_of(sel);
        if s < sel {
            s
        } else if s == 0 {
            start_of(n - 1)
        } else {
            start_of(s - 1)
        }
    }
}

/// First free name of the form "Playlist N" (N from 1).
pub fn new_playlist_name(existing: &[String]) -> String {
    (1..)
        .map(|n| alloc::format!("Playlist {n}"))
        .find(|c| !existing.iter().any(|e| e.eq_ignore_ascii_case(c)))
        .unwrap_or_default()
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
            out.push(AlbumRef::new(artist, v));
        }
    }
    sort_albums(&mut out);
    out.dedup();
    out
}

/// Album counts per artist from a grouped album list, sorted by name.
pub fn artists_from_albums(albums: &[AlbumRef]) -> Vec<ArtistRef> {
    let mut out: Vec<ArtistRef> = Vec::new();
    for a in albums {
        match out.iter_mut().find(|x| x.name == a.artist) {
            Some(x) => x.albums += 1,
            None => out.push(ArtistRef::new(a.artist.clone(), 1)),
        }
    }
    out.sort_by_key(|a| sort_key(&a.name));
    out
}

/// One artist's albums from `list album (albumartist == X) group date`
/// (a `Date:` header applies to the following `Album:` lines). An album
/// with several dates keeps the earliest year.
pub fn parse_dated_albums<'a>(pairs: impl IntoIterator<Item = (&'a str, &'a str)>, artist: &str) -> Vec<AlbumRef> {
    let mut year = None;
    let mut out: Vec<AlbumRef> = Vec::new();
    for (k, v) in pairs {
        if k.eq_ignore_ascii_case("date") {
            year = parse_year(v);
        } else if k.eq_ignore_ascii_case("album") && !v.is_empty() {
            match out.iter_mut().find(|a| a.album == v) {
                Some(a) => a.year = match (a.year, year) {
                    (Some(x), Some(y)) => Some(x.min(y)),
                    (x, y) => x.or(y),
                },
                None => out.push(AlbumRef { year, ..AlbumRef::new(artist, v) }),
            }
        }
    }
    out
}

/// Fill track counts and running time from `count (albumartist == X) group album`.
pub fn apply_album_counts<'a>(albums: &mut [AlbumRef], pairs: impl IntoIterator<Item = (&'a str, &'a str)>) {
    let mut cur: Option<usize> = None;
    for (k, v) in pairs {
        if k.eq_ignore_ascii_case("album") {
            cur = albums.iter().position(|a| a.album == v);
        } else if let Some(i) = cur {
            if k.eq_ignore_ascii_case("songs") {
                albums[i].tracks = v.trim().parse().unwrap_or(0);
            } else if k.eq_ignore_ascii_case("playtime") {
                albums[i].secs = v.trim().parse().unwrap_or(0);
            }
        }
    }
}

/// An artist's albums in release order; albums without a year go last.
pub fn sort_albums_by_year(v: &mut [AlbumRef]) {
    v.sort_by(|a, b| (a.year.unwrap_or(u16::MAX), sort_key(&a.album)).cmp(&(b.year.unwrap_or(u16::MAX), sort_key(&b.album))));
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
    fn years_counts_and_artist_grouping() {
        assert_eq!(parse_year("1958"), Some(1958));
        assert_eq!(parse_year("1958-03-04"), Some(1958));
        assert_eq!(parse_year("03/1958"), Some(1958));
        assert_eq!(parse_year("20070101"), None);
        assert_eq!(parse_year(""), None);
        let pairs = vec![("Date", "1955"), ("Album", "Blue Moods"), ("Album", "Musings"), ("Date", ""), ("Album", "Loose"), ("Date", "1954"), ("Album", "Musings")];
        let mut v = parse_dated_albums(pairs, "Miles Davis");
        apply_album_counts(&mut v, vec![("Album", "Blue Moods"), ("songs", "4"), ("playtime", "1607"), ("Album", "Musings"), ("songs", "6")]);
        sort_albums_by_year(&mut v);
        let o: Vec<_> = v.iter().map(|a| (a.album.as_str(), a.year, a.tracks)).collect();
        assert_eq!(o, [("Musings", Some(1954), 6), ("Blue Moods", Some(1955), 4), ("Loose", None, 0)]);
        assert_eq!(v[1].title_with_year(), "Blue Moods (1955)");
        assert_eq!(v[1].summary(), "4 tracks \u{b7} 26:47");
        assert_eq!(v[0].summary(), "6 tracks");
        assert_eq!(v[2].summary(), "");
        let all = vec![AlbumRef::new("The Police", "Synchronicity"), AlbumRef::new("ABBA", "Arrival"), AlbumRef::new("ABBA", "Voulez-Vous")];
        assert_eq!(artists_from_albums(&all), [ArtistRef::new("ABBA", 2), ArtistRef::new("The Police", 1)]);
        assert_eq!(plural(1, "album"), "1 album");
    }

    #[test]
    fn letters_and_jumps() {
        assert_eq!(index_letter("The Who"), 'W');
        assert_eq!(index_letter("abba"), 'A');
        assert_eq!(index_letter("\u{e9}dith Piaf"), '\u{c9}');
        assert_eq!(index_letter("10cc"), '#');
        assert_eq!(index_letter(""), '#');
        let k: Vec<char> = "##AABBBD".chars().collect();
        assert_eq!(letter_jump(&k, 0, 1), 2);
        assert_eq!(letter_jump(&k, 2, 1), 4);
        assert_eq!(letter_jump(&k, 5, 1), 7);
        assert_eq!(letter_jump(&k, 7, 1), 0); // wraps
        assert_eq!(letter_jump(&k, 6, -1), 4); // start of own group first
        assert_eq!(letter_jump(&k, 4, -1), 2); // then the previous group
        assert_eq!(letter_jump(&k, 1, -1), 0);
        assert_eq!(letter_jump(&k, 0, -1), 7); // wraps to the last group
        assert_eq!(letter_jump(&[], 3, 1), 0);
        let names = vec!["Playlist 1".to_string(), "playlist 2".to_string(), "Road trip".to_string()];
        assert_eq!(new_playlist_name(&names), "Playlist 3");
        assert_eq!(new_playlist_name(&[]), "Playlist 1");
    }

    #[test]
    fn misc() {
        assert_eq!(sort_key("The Who"), "who");
        assert_eq!(fmt_time(3599.9), "59:59");
        assert_eq!(fmt_time(-3.0), "0:00");
        assert_eq!(RepeatMode::Off.next().next().next(), RepeatMode::Off);
    }
}
