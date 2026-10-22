//! Tiny MPD client: text protocol over a UNIX socket or TCP, with socket
//! timeouts so a stuck daemon can never freeze the UI.
//!
//! Connection order: `$MPD_HOST` (path starting with `/`, or host name) and
//! `$MPD_PORT`, then `/run/mpd/socket`, then `127.0.0.1:6600`.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::os::unix::net::UnixStream;
use std::time::Duration;

pub type Pairs = Vec<(String, String)>;

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    /// `ACK [code@index] {command} message`
    Ack(String),
    Protocol(String),
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(e) => write!(f, "mpd i/o: {e}"),
            Error::Ack(m) => write!(f, "mpd: {m}"),
            Error::Protocol(m) => write!(f, "mpd protocol: {m}"),
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

trait Stream: Read + Write + Send {}
impl<T: Read + Write + Send> Stream for T {}

pub struct Client {
    r: BufReader<Box<dyn Stream>>,
    pub version: String,
}

/// Quote an argument for the MPD protocol.
pub fn quote(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        if c == '"' || c == '\\' {
            o.push('\\');
        }
        o.push(c);
    }
    o.push('"');
    o
}

/// Escape a value inside an MPD filter expression, e.g. (artist == 'x').
pub fn filter_eq(tag: &str, value: &str) -> String {
    let mut v = String::new();
    for c in value.chars() {
        if c == '\'' || c == '\\' || c == '"' {
            v.push('\\');
        }
        v.push(c);
    }
    format!("({tag} == '{v}')")
}

impl Client {
    pub fn connect_default() -> Result<Self> {
        let timeout = Some(Duration::from_secs(3));
        if let Ok(h) = std::env::var("MPD_HOST") {
            if h.starts_with('/') {
                return Self::from_unix(UnixStream::connect(&h)?, timeout);
            }
            let port = std::env::var("MPD_PORT").unwrap_or_else(|_| "6600".into());
            return Self::from_tcp(TcpStream::connect(format!("{h}:{port}"))?, timeout);
        }
        if let Ok(s) = UnixStream::connect("/run/mpd/socket") {
            return Self::from_unix(s, timeout);
        }
        Self::from_tcp(TcpStream::connect("127.0.0.1:6600")?, timeout)
    }

    fn from_unix(s: UnixStream, t: Option<Duration>) -> Result<Self> {
        s.set_read_timeout(t)?;
        s.set_write_timeout(t)?;
        Self::handshake(Box::new(s))
    }

    fn from_tcp(s: TcpStream, t: Option<Duration>) -> Result<Self> {
        s.set_read_timeout(t)?;
        s.set_write_timeout(t)?;
        let _ = s.set_nodelay(true);
        Self::handshake(Box::new(s))
    }

    fn handshake(s: Box<dyn Stream>) -> Result<Self> {
        let mut r = BufReader::with_capacity(16 * 1024, s);
        let mut line = String::new();
        r.read_line(&mut line)?;
        let v = line.trim_end().strip_prefix("OK MPD ").ok_or_else(|| Error::Protocol(line.clone()))?;
        Ok(Client { version: v.to_string(), r })
    }

    fn send(&mut self, cmd: &str) -> Result<()> {
        let s = self.r.get_mut();
        s.write_all(cmd.as_bytes())?;
        s.write_all(b"\n")?;
        s.flush()?;
        Ok(())
    }

    fn read_line(&mut self) -> Result<String> {
        let mut buf = Vec::new();
        let n = self.r.read_until(b'\n', &mut buf)?;
        if n == 0 {
            return Err(Error::Io(io::Error::new(io::ErrorKind::UnexpectedEof, "mpd closed")));
        }
        if buf.last() == Some(&b'\n') {
            buf.pop();
        }
        Ok(String::from_utf8_lossy(&buf).into_owned())
    }

    /// Run a command; returns key/value pairs until OK.
    pub fn cmd(&mut self, cmd: &str) -> Result<Pairs> {
        self.send(cmd)?;
        let mut out = Vec::new();
        loop {
            let l = self.read_line()?;
            if l == "OK" {
                return Ok(out);
            }
            if l.starts_with("ACK ") {
                return Err(Error::Ack(l));
            }
            if let Some((k, v)) = l.split_once(": ") {
                out.push((k.to_string(), v.to_string()));
            } else {
                return Err(Error::Protocol(l));
            }
        }
    }

    /// Binary-chunk commands (`albumart`, `readpicture`): fetch the whole blob.
    /// Returns None when MPD has no picture. Stops at `max` bytes.
    pub fn binary(&mut self, command: &str, uri: &str, max: usize) -> Result<Option<Vec<u8>>> {
        let mut data = Vec::new();
        loop {
            self.send(&format!("{command} {} {}", quote(uri), data.len()))?;
            let mut size: Option<usize> = None;
            let chunk: Option<usize>;
            loop {
                let l = self.read_line()?;
                if l == "OK" {
                    // readpicture with no picture -> just OK
                    return Ok(if data.is_empty() { None } else { Some(data) });
                }
                if l.starts_with("ACK ") {
                    // "No file exists" etc.
                    return if l.contains("No file exists") || l.contains("No such") || data.is_empty() {
                        Ok(None)
                    } else {
                        Err(Error::Ack(l))
                    };
                }
                if let Some(v) = l.strip_prefix("size: ") {
                    size = v.parse().ok();
                } else if let Some(v) = l.strip_prefix("binary: ") {
                    chunk = v.parse().ok();
                    break;
                }
            }
            let (size, n) = match (size, chunk) {
                (Some(s), Some(n)) => (s, n),
                _ => return Err(Error::Protocol("binary response without size".into())),
            };
            if size > max {
                // drain and give up
                let mut sink = vec![0u8; n + 1];
                self.r.read_exact(&mut sink)?;
                let _ = self.read_line()?;
                return Ok(None);
            }
            let start = data.len();
            data.resize(start + n, 0);
            self.r.read_exact(&mut data[start..])?;
            let nl = self.read_line()?; // newline after binary
            if !nl.is_empty() {
                return Err(Error::Protocol("missing newline after binary".into()));
            }
            let ok = self.read_line()?;
            if ok != "OK" {
                return Err(Error::Protocol(ok));
            }
            if n == 0 || data.len() >= size {
                return Ok(Some(data));
            }
        }
    }

    // ---- typed helpers ----------------------------------------------------
    pub fn status(&mut self) -> Result<Status> {
        Ok(Status::from_pairs(&self.cmd("status")?))
    }
    pub fn current_song(&mut self) -> Result<Option<Song>> {
        Ok(Song::parse_list(&self.cmd("currentsong")?).into_iter().next())
    }
    pub fn queue(&mut self) -> Result<Vec<Song>> {
        Ok(Song::parse_list(&self.cmd("playlistinfo")?))
    }
    /// Values of a tag (e.g. `list albumartist`), deduplicated, sorted.
    pub fn list(&mut self, what: &str, filter: Option<&str>) -> Result<Vec<String>> {
        let c = match filter {
            Some(f) => format!("list {what} {}", quote(f)),
            None => format!("list {what}"),
        };
        let key = what.to_ascii_lowercase();
        let mut v: Vec<String> = self
            .cmd(&c)?
            .into_iter()
            .filter(|(k, _)| k.to_ascii_lowercase() == key)
            .map(|(_, v)| v)
            .filter(|v| !v.is_empty())
            .collect();
        sort_names(&mut v);
        v.dedup();
        Ok(v)
    }
    pub fn find(&mut self, filter: &str) -> Result<Vec<Song>> {
        Ok(Song::parse_list(&self.cmd(&format!("find {}", quote(filter)))?))
    }
    pub fn playlists(&mut self) -> Result<Vec<String>> {
        let mut v: Vec<String> =
            self.cmd("listplaylists")?.into_iter().filter(|(k, _)| k == "playlist").map(|(_, v)| v).collect();
        sort_names(&mut v);
        Ok(v)
    }
    pub fn playlist_songs(&mut self, name: &str) -> Result<Vec<Song>> {
        Ok(Song::parse_list(&self.cmd(&format!("listplaylistinfo {}", quote(name)))?))
    }
    pub fn simple(&mut self, c: &str) -> Result<()> {
        self.cmd(c).map(|_| ())
    }
    /// Replace the queue with `uris` and start at `start`.
    pub fn play_uris(&mut self, uris: &[String], start: usize) -> Result<()> {
        let mut batch = String::from("command_list_begin\nclear\n");
        for u in uris {
            batch.push_str(&format!("add {}\n", quote(u)));
        }
        batch.push_str(&format!("play {start}\ncommand_list_end"));
        self.simple(&batch)
    }
    pub fn add_uris(&mut self, uris: &[String]) -> Result<()> {
        let mut batch = String::from("command_list_begin\n");
        for u in uris {
            batch.push_str(&format!("add {}\n", quote(u)));
        }
        batch.push_str("command_list_end");
        self.simple(&batch)
    }
}

/// Case-insensitive sort that ignores a leading "The ".
pub fn sort_names(v: &mut [String]) {
    v.sort_by_key(|a| sort_key(a));
}

pub fn sort_key(s: &str) -> String {
    let l = s.to_lowercase();
    l.strip_prefix("the ").map(|x| x.to_string()).unwrap_or(l)
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    pub state: String, // play | pause | stop
    pub volume: Option<u8>,
    pub repeat: bool,
    pub random: bool,
    pub single: bool,
    pub song: Option<usize>,
    pub playlist_len: usize,
    pub elapsed: f32,
    pub duration: f32,
    pub updating_db: bool,
    pub error: Option<String>,
}

impl Status {
    pub fn from_pairs(p: &Pairs) -> Self {
        let mut s = Status::default();
        for (k, v) in p {
            match k.as_str() {
                "state" => s.state = v.clone(),
                "volume" => s.volume = v.parse::<i32>().ok().filter(|x| *x >= 0).map(|x| x.min(100) as u8),
                "repeat" => s.repeat = v == "1",
                "random" => s.random = v == "1",
                "single" => s.single = v == "1",
                "song" => s.song = v.parse().ok(),
                "playlistlength" => s.playlist_len = v.parse().unwrap_or(0),
                "elapsed" => s.elapsed = v.parse().unwrap_or(0.0),
                "duration" => s.duration = v.parse().unwrap_or(0.0),
                "time" if s.duration == 0.0 => {
                    if let Some((_, d)) = v.split_once(':') {
                        s.duration = d.parse().unwrap_or(0.0);
                    }
                }
                "updating_db" => s.updating_db = true,
                "error" => s.error = Some(v.clone()),
                _ => {}
            }
        }
        s
    }
    pub fn playing(&self) -> bool {
        self.state == "play"
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Song {
    pub file: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album_artist: Option<String>,
    pub album: Option<String>,
    pub track: Option<u32>,
    pub disc: Option<u32>,
    pub date: Option<String>,
    pub duration: f32,
    pub pos: Option<usize>,
}

fn leading_num(v: &str) -> Option<u32> {
    // "3/12" or "03" or "3 of 12"
    let d: String = v.trim().chars().take_while(|c| c.is_ascii_digit()).collect();
    d.parse().ok()
}

impl Song {
    /// Parse `file:`-delimited song records (lsinfo/find/playlistinfo).
    pub fn parse_list(p: &Pairs) -> Vec<Song> {
        let mut out: Vec<Song> = Vec::new();
        for (k, v) in p {
            if k == "file" {
                out.push(Song { file: v.clone(), ..Default::default() });
                continue;
            }
            if k == "directory" || k == "playlist" {
                continue;
            }
            let Some(s) = out.last_mut() else { continue };
            match k.as_str() {
                "Title" => s.title = Some(v.clone()),
                "Artist" if s.artist.is_none() => s.artist = Some(v.clone()),
                "AlbumArtist" if s.album_artist.is_none() => s.album_artist = Some(v.clone()),
                "Album" => s.album = Some(v.clone()),
                "Track" => s.track = leading_num(v),
                "Disc" => s.disc = leading_num(v),
                "Date" => s.date = Some(v.clone()),
                "duration" => s.duration = v.parse().unwrap_or(0.0),
                "Time" if s.duration == 0.0 => s.duration = v.parse().unwrap_or(0.0),
                "Pos" => s.pos = v.parse().ok(),
                _ => {}
            }
        }
        out
    }

    /// Display title: Title tag, else file name without extension.
    pub fn display_title(&self) -> String {
        if let Some(t) = &self.title {
            return t.clone();
        }
        let name = self.file.rsplit('/').next().unwrap_or(&self.file);
        name.rsplit_once('.').map(|(a, _)| a).unwrap_or(name).to_string()
    }

    pub fn display_artist(&self) -> &str {
        self.artist.as_deref().or(self.album_artist.as_deref()).unwrap_or("Unknown artist")
    }

    /// Album grouping artist (AlbumArtist, else Artist).
    pub fn group_artist(&self) -> &str {
        self.album_artist.as_deref().or(self.artist.as_deref()).unwrap_or("Unknown artist")
    }
}

/// Sort an album's tracks by disc, track number, then file name.
pub fn sort_album_tracks(v: &mut [Song]) {
    v.sort_by(|a, b| {
        (a.disc.unwrap_or(1), a.track.unwrap_or(u32::MAX), &a.file).cmp(&(b.disc.unwrap_or(1), b.track.unwrap_or(u32::MAX), &b.file))
    });
}

pub fn fmt_time(secs: f32) -> String {
    let s = secs.max(0.0) as u32;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    fn pairs(s: &str) -> Pairs {
        s.lines().filter_map(|l| l.split_once(": ")).map(|(a, b)| (a.into(), b.into())).collect()
    }

    #[test]
    fn parses_songs_and_sorts_tracks() {
        let p = pairs(
            "file: A/Album/02 Two.flac\nTitle: Two\nArtist: A\nAlbumArtist: A\nAlbum: Album\nTrack: 2/10\nDisc: 1\nduration: 61.5\n\
             file: A/Album/01 One.flac\nTitle: One\nArtist: A\nAlbum: Album\nTrack: 01\nduration: 30.0\n\
             file: A/Album/x.mp3\nDisc: 2/2\nTrack: 1\n\
             directory: A/Album/scans\n",
        );
        let mut v = Song::parse_list(&p);
        assert_eq!(v.len(), 3);
        assert_eq!(v[0].track, Some(2));
        assert_eq!(v[0].duration, 61.5);
        assert_eq!(v[2].display_title(), "x");
        assert_eq!(v[1].group_artist(), "A");
        sort_album_tracks(&mut v);
        let order: Vec<_> = v.iter().map(|s| s.display_title()).collect();
        assert_eq!(order, ["One", "Two", "x"]);
    }

    #[test]
    fn parses_status() {
        let s = Status::from_pairs(&pairs(
            "volume: 42\nrepeat: 1\nrandom: 0\nsingle: 1\nstate: play\nsong: 3\nplaylistlength: 9\nelapsed: 12.345\nduration: 200.1\n",
        ));
        assert!(s.playing() && s.repeat && s.single && !s.random);
        assert_eq!((s.volume, s.song, s.playlist_len), (Some(42), Some(3), 9));
        let s2 = Status::from_pairs(&pairs("volume: -1\nstate: stop\ntime: 3:180\n"));
        assert_eq!((s2.volume, s2.duration), (None, 180.0));
    }

    #[test]
    fn quoting_and_sort() {
        assert_eq!(quote(r#"a "b" \c"#), r#""a \"b\" \\c""#);
        assert_eq!(filter_eq("album", "It's"), r"(album == 'It\'s')");
        let mut v = vec!["the Beatles".to_string(), "ABBA".into(), "beck".into()];
        sort_names(&mut v);
        assert_eq!(v, ["ABBA", "the Beatles", "beck"]);
        assert_eq!(fmt_time(3725.0), "1:02:05");
        assert_eq!(fmt_time(65.9), "1:05");
    }

    /// Fake MPD server: checks request framing and binary chunk handling.
    #[test]
    fn fake_server_albumart_chunks() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let art: Vec<u8> = (0..10_000u32).map(|i| (i % 251) as u8).collect();
        let art2 = art.clone();
        let t = std::thread::spawn(move || {
            let (s, _) = l.accept().unwrap();
            let mut w = s.try_clone().unwrap();
            let mut r = BufReader::new(s);
            w.write_all(b"OK MPD 0.24.0\n").unwrap();
            let mut line = String::new();
            loop {
                line.clear();
                if r.read_line(&mut line).unwrap() == 0 {
                    break;
                }
                let l = line.trim_end();
                if l.starts_with("albumart") {
                    let off: usize = l.rsplit(' ').next().unwrap().parse().unwrap();
                    let n = (art2.len() - off).min(4096);
                    write!(w, "size: {}\nbinary: {}\n", art2.len(), n).unwrap();
                    w.write_all(&art2[off..off + n]).unwrap();
                    w.write_all(b"\nOK\n").unwrap();
                } else if l.starts_with("readpicture") {
                    w.write_all(b"OK\n").unwrap();
                } else if l == "status" {
                    w.write_all(b"state: pause\nvolume: 10\nOK\n").unwrap();
                } else {
                    w.write_all(b"ACK [5@0] {} unknown command\n").unwrap();
                }
            }
        });
        std::env::set_var("MPD_HOST", "127.0.0.1");
        std::env::set_var("MPD_PORT", addr.port().to_string());
        let mut c = Client::connect_default().unwrap();
        assert_eq!(c.version, "0.24.0");
        assert_eq!(c.binary("albumart", "x/y.flac", 1 << 20).unwrap().unwrap(), art);
        assert!(c.binary("readpicture", "x/y.flac", 1 << 20).unwrap().is_none());
        assert_eq!(c.status().unwrap().state, "pause");
        assert!(matches!(c.cmd("bogus"), Err(Error::Ack(_))));
        drop(c);
        t.join().unwrap();
    }
}
