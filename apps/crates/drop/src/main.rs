//! zpod-drop: the ZPOD's tiny web server (M6: offline drop box + OPDS).
//!
//! Drop box (`dropbox = on` in /etc/zpod/drop.conf; Tools > Drop box):
//!   GET  /                 home page: browse, upload, OPDS address
//!   GET  /files/<area>/... browse/download music, books or the inbox
//!   PUT  /inbox/<name>     upload any file into /data/inbox (size-capped)
//!   PUT  /music/<path>     store one music file (atomic: .part + rename)
//!   POST /update           ask MPD to rescan (also done automatically after uploads)
//! Book catalog (`opds = on`; Tools > Book catalog), see [`opds`]:
//!   GET  /opds...          OPDS 1.2 feeds of /data/books
//!   GET  /books/<path>, /cover/<path>
//! Always: GET /status (JSON).
//!
//! Both are OFF by default; the launcher turns the socket on when either is
//! enabled. Access: peers on the USB gadget subnet (10.12.194.0/28) and on
//! the ZPOD's own hotspot (10.42.0.0/24, WPA2 with a passphrase made on the
//! device) are trusted; any other peer (home Wi-Fi) must send the token
//! from drop.conf as header `X-Zpod-Token`, `?token=`, or as the HTTP Basic
//! password (any user name; what OPDS reader apps support). No token => no
//! home Wi-Fi access.
//! Socket activation: if LISTEN_FDS=1 (systemd .socket), serve fd 3 and exit
//! after 10 idle minutes; otherwise bind `listen` from the config.

mod epub;
mod opds;

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, Ipv4Addr, TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const MAX_FILE: u64 = 2 << 30;
/// Space always left free on /data after an upload.
const RESERVE: u64 = 64 << 20;
const MAX_CONN: usize = 4;
const IDLE_EXIT: Duration = Duration::from_secs(600);

#[derive(Clone, Debug)]
struct Config {
    listen: String,
    music_dir: PathBuf,
    books_dir: PathBuf,
    inbox_dir: PathBuf,
    token: Option<String>,
    usb_net: (Ipv4Addr, u8),
    hotspot_net: (Ipv4Addr, u8),
    dropbox: bool,
    opds: bool,
    /// per-file upload limit for the inbox (bytes)
    max_upload: u64,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            listen: "0.0.0.0:8080".into(),
            music_dir: "/srv/zpod/media/music".into(),
            books_dir: "/data/books".into(),
            inbox_dir: "/data/inbox".into(),
            token: None,
            usb_net: (Ipv4Addr::new(10, 12, 194, 0), 28),
            hotspot_net: (Ipv4Addr::new(10, 42, 0, 0), 24),
            dropbox: false,
            opds: false,
            max_upload: 1 << 30,
        }
    }
}

fn parse_config(text: &str) -> Config {
    let mut c = Config::default();
    for line in text.lines() {
        let l = line.split('#').next().unwrap_or("").trim();
        let Some((k, v)) = l.split_once('=') else { continue };
        let v = v.trim().trim_matches('"').to_string();
        match k.trim() {
            "listen" => c.listen = v,
            "music_dir" => c.music_dir = v.into(),
            "books_dir" => c.books_dir = v.into(),
            "inbox_dir" => c.inbox_dir = v.into(),
            "dropbox" => c.dropbox = on(&v),
            "opds" => c.opds = on(&v),
            "max_upload_mb" => c.max_upload = v.parse::<u64>().map(|m| m.clamp(1, 2048) << 20).unwrap_or(c.max_upload),
            "token" if v.len() >= 8 => c.token = Some(v),
            _ => {}
        }
    }
    c
}

fn on(v: &str) -> bool {
    matches!(v.to_ascii_lowercase().as_str(), "on" | "yes" | "true" | "1")
}

fn in_net(ip: IpAddr, (net, bits): (Ipv4Addr, u8)) -> bool {
    match ip {
        IpAddr::V4(a) => {
            let mask = if bits == 0 { 0 } else { u32::MAX << (32 - bits) };
            u32::from(a) & mask == u32::from(net) & mask
        }
        IpAddr::V6(a) => a.to_ipv4_mapped().is_some_and(|v4| in_net(IpAddr::V4(v4), (net, bits))),
    }
}

pub(crate) fn pct_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => {
                let h = std::str::from_utf8(&b[i + 1..i + 3]).ok()?;
                out.push(u8::from_str_radix(h, 16).ok()?);
                i += 3;
            }
            b'%' => return None,
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

const ALLOWED_EXT: &[&str] = &[
    "mp3", "flac", "ogg", "oga", "opus", "m4a", "aac", "wav", "aif", "aiff", "wv", "ape", "mpc", "jpg", "jpeg", "png", "m3u", "m3u8", "cue",
];

/// Relative music path from a URL path: no traversal, no hidden parts,
/// audio/art/playlist extensions only.
fn safe_music_path(raw: &str) -> Result<PathBuf, &'static str> {
    let dec = pct_decode(raw).ok_or("bad percent-encoding")?;
    if dec.len() > 1024 || dec.contains('\0') || dec.contains('\\') {
        return Err("bad path");
    }
    let p = Path::new(&dec);
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::Normal(s) => {
                let s = s.to_str().ok_or("bad path")?;
                if s.starts_with('.') || s.len() > 255 {
                    return Err("hidden or overlong path component");
                }
                out.push(s);
            }
            Component::CurDir => {}
            _ => return Err("path traversal"),
        }
    }
    let ext = out.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).ok_or("no file extension")?;
    if !ALLOWED_EXT.contains(&ext.as_str()) {
        return Err("file type not allowed");
    }
    Ok(out)
}

/// Relative path for browsing/downloading: no traversal, no hidden parts
/// (any extension). Empty = the area root.
fn safe_rel(raw: &str) -> Result<PathBuf, &'static str> {
    let dec = pct_decode(raw).ok_or("bad percent-encoding")?;
    if dec.len() > 1024 || dec.contains('\0') || dec.contains('\\') {
        return Err("bad path");
    }
    let mut out = PathBuf::new();
    for c in Path::new(&dec).components() {
        match c {
            Component::Normal(s) => {
                let s = s.to_str().ok_or("bad path")?;
                if s.starts_with('.') || s.len() > 255 {
                    return Err("hidden or overlong path component");
                }
                out.push(s);
            }
            Component::CurDir => {}
            _ => return Err("path traversal"),
        }
    }
    Ok(out)
}

/// One inbox file name (no directories, not hidden, sane length).
fn safe_inbox_name(raw: &str) -> Result<String, &'static str> {
    let n = pct_decode(raw).ok_or("bad percent-encoding")?;
    if n.chars().any(|c| c.is_control()) {
        return Err("bad file name");
    }
    let n = n.trim();
    if n.is_empty() || n.len() > 200 || n.starts_with('.') || n.contains(['/', '\\', '\0']) || n.chars().any(|c| c.is_control()) {
        return Err("bad file name");
    }
    Ok(n.to_string())
}

/// "name.ext" -> "name (2).ext" until free.
fn unique(dir: &Path, name: &str) -> PathBuf {
    let p = dir.join(name);
    if !p.exists() {
        return p;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.to_string(), String::new()),
    };
    (2..10_000).map(|i| dir.join(format!("{stem} ({i}){ext}"))).find(|p| !p.exists()).unwrap_or(p)
}

fn b64_decode(s: &str) -> Option<Vec<u8>> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32)
    };
    let b: Vec<u8> = s.bytes().filter(|c| *c != b'=').collect();
    let mut out = Vec::new();
    for ch in b.chunks(4) {
        let mut n = 0u32;
        for (i, c) in ch.iter().enumerate() {
            n |= val(*c)? << (18 - 6 * i);
        }
        let bytes = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
        out.extend_from_slice(&bytes[..ch.len().saturating_sub(1)]);
    }
    Some(out)
}

fn content_type(p: &Path) -> &'static str {
    if let Some(m) = opds::mime_of(p) {
        return m;
    }
    match p.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref() {
        Some("mp3") => "audio/mpeg",
        Some("flac") => "audio/flac",
        Some("ogg" | "oga" | "opus") => "audio/ogg",
        Some("m4a" | "aac") => "audio/mp4",
        Some("wav") => "audio/wav",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("m3u" | "m3u8" | "cue" | "log" | "md") => "text/plain; charset=utf-8",
        Some("html" | "htm") => "text/plain; charset=utf-8", // never render uploaded HTML
        _ => "application/octet-stream",
    }
}

fn esc(s: &str) -> String {
    opds::xml(s)
}

struct Req {
    method: String,
    path: String,
    query: String,
    headers: Vec<(String, String)>,
}

impl Req {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
    fn query_param(&self, name: &str) -> Option<String> {
        self.query.split('&').find_map(|kv| {
            let (k, v) = kv.split_once('=')?;
            (k == name).then(|| pct_decode(v)).flatten()
        })
    }
}

fn read_req(r: &mut BufReader<&TcpStream>) -> Option<Req> {
    let mut line = String::new();
    r.by_ref().take(8192).read_line(&mut line).ok()?;
    let mut it = line.split_whitespace();
    let method = it.next()?.to_string();
    let target = it.next()?.to_string();
    let (path, query) = match target.split_once('?') {
        Some((p, q)) => (p.to_string(), q.to_string()),
        None => (target, String::new()),
    };
    let mut headers = Vec::new();
    loop {
        let mut h = String::new();
        if r.by_ref().take(8192).read_line(&mut h).ok()? == 0 {
            return None;
        }
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            headers.push((k.trim().to_string(), v.trim().to_string()));
        }
        if headers.len() > 64 {
            return None;
        }
    }
    Some(Req { method, path, query, headers })
}

fn respond(s: &mut TcpStream, code: u16, ctype: &str, body: &[u8]) {
    respond_h(s, code, ctype, body, "")
}

fn respond_h(s: &mut TcpStream, code: u16, ctype: &str, body: &[u8], extra: &str) {
    let reason = match code {
        200 => "OK",
        201 => "Created",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        411 => "Length Required",
        413 => "Payload Too Large",
        500 => "Internal Server Error",
        507 => "Insufficient Storage",
        _ => "",
    };
    let _ = write!(s, "HTTP/1.1 {code} {reason}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n{extra}\r\n", body.len());
    let _ = s.write_all(body);
}

/// Stream a file (GET); HEAD sends only the headers.
fn send_file(s: &mut TcpStream, p: &Path, head_only: bool) {
    let Ok(mut f) = std::fs::File::open(p) else { return respond(s, 404, "text/plain", b"404\n") };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let _ = write!(
        s,
        "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {len}\r\nConnection: close\r\nX-Content-Type-Options: nosniff\r\nContent-Disposition: inline; filename*=UTF-8''{}\r\n\r\n",
        content_type(p),
        opds::href_path(&name)
    );
    if !head_only {
        let _ = std::io::copy(&mut f, s);
    }
}

fn free_bytes(p: &Path) -> u64 {
    let c = std::ffi::CString::new(p.as_os_str().as_encoded_bytes()).unwrap_or_default();
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        return 0;
    }
    st.f_bavail as u64 * st.f_frsize as u64
}

struct Shared {
    cfg: Config,
    books: Mutex<HashMap<PathBuf, (i64, u64, opds::Book)>>,
    last_upload: Mutex<Option<Instant>>,
    last_activity: AtomicU64,
    active: AtomicUsize,
    t0: Instant,
}

fn handle(mut s: TcpStream, sh: &Shared) {
    let _ = s.set_read_timeout(Some(Duration::from_secs(30)));
    let peer = s.peer_addr().map(|a| a.ip()).unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
    let rs = s.try_clone().unwrap();
    let mut r = BufReader::with_capacity(64 * 1024, &rs);
    let Some(req) = read_req(&mut r) else { return };
    let trusted = in_net(peer, sh.cfg.usb_net) || in_net(peer, sh.cfg.hotspot_net) || peer.is_loopback();
    let basic_pw = req.header("Authorization").and_then(|a| a.strip_prefix("Basic ")).and_then(|b| b64_decode(b.trim())).and_then(|v| String::from_utf8(v).ok()).and_then(|up| up.split_once(':').map(|(_, p)| p.to_string()));
    let qtok = req.query_param("token");
    let token_ok = sh.cfg.token.as_deref().is_some_and(|t| req.header("X-Zpod-Token") == Some(t) || qtok.as_deref() == Some(t) || basic_pw.as_deref() == Some(t));
    if !trusted && !token_ok {
        if sh.cfg.token.is_some() {
            // OPDS readers ask for a user name + password: any name, password = token
            return respond_h(&mut s, 401, "text/plain", b"401: token required on Wi-Fi (password = token from /etc/zpod/drop.conf)\n", "WWW-Authenticate: Basic realm=\"ZPOD\"\r\n");
        }
        let msg = "403: home Wi-Fi access is off; use the USB cable or the ZPOD hotspot, or set token= in /etc/zpod/drop.conf\n";
        respond(&mut s, 403, "text/plain", msg.as_bytes());
        return;
    }
    // links keep ?token= for clients that authenticated that way
    let q = match (&qtok, token_ok && !trusted) {
        (Some(t), true) => format!("?token={}", opds::href_path(t)),
        _ => String::new(),
    };
    let (m, path) = (req.method.as_str(), req.path.as_str());
    let get = m == "GET" || m == "HEAD";
    let off = |s: &mut TcpStream, what: &str| respond(s, 404, "text/plain", format!("404: the {what} is off (turn it on in Tools on the ZPOD)\n").as_bytes());
    match (m, path) {
        ("GET", "/status") => {
            let body = format!(
                "{{\"free_bytes\":{},\"music_dir\":\"{}\",\"dropbox\":{},\"opds\":{},\"max_upload\":{},\"core\":\"{}\"}}\n",
                free_bytes(&sh.cfg.music_dir),
                sh.cfg.music_dir.display(),
                sh.cfg.dropbox,
                sh.cfg.opds,
                sh.cfg.max_upload,
                env!("CARGO_PKG_VERSION")
            );
            respond(&mut s, 200, "application/json", body.as_bytes());
        }
        (_, p) if get && (p == "/opds" || p.starts_with("/opds/")) => {
            if !sh.cfg.opds {
                return off(&mut s, "book catalog");
            }
            let books = opds::scan(&sh.cfg.books_dir, &mut sh.books.lock().unwrap());
            let body = match p.trim_end_matches('/') {
                "/opds" => opds::navigation(&books, &q),
                "/opds/all" => opds::acquisition(&books, "all", "All books", "/opds/all", &q),
                "/opds/new" => {
                    let mut b = books.clone();
                    b.sort_by(|x, y| y.mtime.cmp(&x.mtime));
                    opds::acquisition(&b, "new", "Newest", "/opds/new", &q)
                }
                "/opds/authors" => opds::authors(&books, &q),
                a if a.starts_with("/opds/author/") => {
                    let name = pct_decode(&a["/opds/author/".len()..]).unwrap_or_default();
                    let b: Vec<opds::Book> = books.into_iter().filter(|b| b.author == name || (b.author.is_empty() && name == "Unknown author")).collect();
                    opds::acquisition(&b, "author", &name, a, &q)
                }
                _ => return respond(&mut s, 404, "text/plain", b"404\n"),
            };
            respond(&mut s, 200, "application/atom+xml;profile=opds-catalog; charset=utf-8", body.as_bytes());
        }
        (_, p) if get && (p.starts_with("/books/") || p.starts_with("/cover/")) => {
            if !(sh.cfg.opds || sh.cfg.dropbox) {
                return off(&mut s, "book catalog");
            }
            let rel = match safe_rel(&p[7..]) {
                Ok(r) if !r.as_os_str().is_empty() => r,
                _ => return respond(&mut s, 400, "text/plain", b"400: bad path\n"),
            };
            let f = sh.cfg.books_dir.join(&rel);
            if p.starts_with("/books/") {
                if opds::mime_of(&f).is_none() || !f.is_file() {
                    return respond(&mut s, 404, "text/plain", b"404\n");
                }
                eprintln!("zpod-drop: {peer} book {}", rel.display());
                send_file(&mut s, &f, m == "HEAD");
            } else {
                match epub::read_cover(&f) {
                    Some((img, mt)) => respond_h(&mut s, 200, &mt, &img, "Cache-Control: max-age=86400\r\n"),
                    None => respond(&mut s, 404, "text/plain", b"404: no cover\n"),
                }
            }
        }
        _ if !sh.cfg.dropbox => off(&mut s, "drop box"),
        (_, "/") if get => {
            let page = PAGE.replace("CSSHERE", CSS).replace("{{MAX}}", &(sh.cfg.max_upload >> 20).to_string()).replace("{{OPDS}}", if sh.cfg.opds { "" } else { "hidden" });
            respond(&mut s, 200, "text/html; charset=utf-8", page.as_bytes());
        }
        (_, p) if get && p.starts_with("/files/") => {
            let rest = &p["/files/".len()..];
            let (area, sub) = rest.split_once('/').unwrap_or((rest, ""));
            let root = match area {
                "music" => &sh.cfg.music_dir,
                "books" => &sh.cfg.books_dir,
                "inbox" => &sh.cfg.inbox_dir,
                _ => return respond(&mut s, 404, "text/plain", b"404\n"),
            };
            let rel = match safe_rel(sub) {
                Ok(r) => r,
                Err(e) => return respond(&mut s, 400, "text/plain", format!("400: {e}\n").as_bytes()),
            };
            let target = root.join(&rel);
            if target.is_dir() {
                let body = listing(area, &rel, &target, &q);
                respond(&mut s, 200, "text/html; charset=utf-8", body.as_bytes());
            } else if target.is_file() {
                eprintln!("zpod-drop: {peer} download {area}/{}", rel.display());
                send_file(&mut s, &target, m == "HEAD");
            } else {
                respond(&mut s, 404, "text/plain", b"404\n");
            }
        }
        ("PUT", p) if p.starts_with("/inbox/") => {
            let name = match safe_inbox_name(&p["/inbox/".len()..]) {
                Ok(n) => n,
                Err(e) => return respond(&mut s, 400, "text/plain", format!("400: {e}\n").as_bytes()),
            };
            let Some(len) = req.header("Content-Length").and_then(|v| v.parse::<u64>().ok()) else {
                return respond(&mut s, 411, "text/plain", b"411: Content-Length required\n");
            };
            if len > sh.cfg.max_upload {
                return respond(&mut s, 413, "text/plain", format!("413: over the {} MB limit\n", sh.cfg.max_upload >> 20).as_bytes());
            }
            let _ = std::fs::create_dir_all(&sh.cfg.inbox_dir);
            if len + RESERVE > free_bytes(&sh.cfg.inbox_dir) {
                return respond(&mut s, 507, "text/plain", b"507: not enough space\n");
            }
            if req.header("Expect").is_some_and(|e| e.eq_ignore_ascii_case("100-continue")) {
                let _ = s.write_all(b"HTTP/1.1 100 Continue\r\n\r\n");
            }
            let dest = unique(&sh.cfg.inbox_dir, &name);
            let fname = dest.file_name().unwrap().to_string_lossy().into_owned();
            match store(&mut r, &sh.cfg.inbox_dir, Path::new(&fname), len) {
                Ok(()) => {
                    eprintln!("zpod-drop: {peer} inbox {fname} ({len} bytes)");
                    respond(&mut s, 201, "text/plain", format!("stored inbox/{fname}\n").as_bytes());
                }
                Err(e) => respond(&mut s, 500, "text/plain", format!("500: {e}\n").as_bytes()),
            }
        }
        ("POST", "/update") => {
            let ok = mpd_update();
            respond(&mut s, if ok { 200 } else { 500 }, "text/plain", if ok { b"update started\n" } else { b"mpd not reachable\n" });
        }
        ("PUT", p) if p.starts_with("/music/") => {
            let rel = match safe_music_path(&p["/music/".len()..]) {
                Ok(r) => r,
                Err(e) => return respond(&mut s, 400, "text/plain", format!("400: {e}\n").as_bytes()),
            };
            let Some(len) = req.header("Content-Length").and_then(|v| v.parse::<u64>().ok()) else {
                return respond(&mut s, 411, "text/plain", b"411: Content-Length required\n");
            };
            if len > MAX_FILE {
                return respond(&mut s, 413, "text/plain", b"413: file too large\n");
            }
            if len + RESERVE > free_bytes(&sh.cfg.music_dir) {
                return respond(&mut s, 507, "text/plain", b"507: not enough space\n");
            }
            if req.header("Expect").is_some_and(|e| e.eq_ignore_ascii_case("100-continue")) {
                let _ = s.write_all(b"HTTP/1.1 100 Continue\r\n\r\n");
            }
            match store(&mut r, &sh.cfg.music_dir, &rel, len) {
                Ok(()) => {
                    *sh.last_upload.lock().unwrap() = Some(Instant::now());
                    eprintln!("zpod-drop: {peer} stored {} ({len} bytes)", rel.display());
                    respond(&mut s, 201, "text/plain", b"stored\n");
                }
                Err(e) => {
                    eprintln!("zpod-drop: {peer} failed {}: {e}", rel.display());
                    respond(&mut s, 500, "text/plain", format!("500: {e}\n").as_bytes());
                }
            }
        }
        _ => respond(&mut s, 404, "text/plain", b"404\n"),
    }
}

fn human(b: u64) -> String {
    match b {
        b if b >= 1 << 30 => format!("{:.1} GB", b as f64 / (1u64 << 30) as f64),
        b if b >= 1 << 20 => format!("{:.1} MB", b as f64 / (1u64 << 20) as f64),
        b if b >= 1 << 10 => format!("{} KB", b >> 10),
        b => format!("{b} B"),
    }
}

/// HTML directory listing (folders first, then files, by name).
fn listing(area: &str, rel: &Path, dir: &Path, q: &str) -> String {
    let mut ents: Vec<(bool, String, u64)> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| {
                    let n = e.file_name().to_string_lossy().into_owned();
                    if n.starts_with('.') {
                        return None;
                    }
                    let m = e.metadata().ok()?;
                    Some((m.is_dir(), n, m.len()))
                })
                .collect()
        })
        .unwrap_or_default();
    ents.sort_by(|a, b| (!a.0, a.1.to_lowercase()).cmp(&(!b.0, b.1.to_lowercase())));
    let relp = rel.to_string_lossy().replace('\\', "/");
    let base = if relp.is_empty() { format!("/files/{area}") } else { format!("/files/{area}/{}", opds::href_path(&relp)) };
    let mut h = format!("<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>ZPOD {area}</title><style>{CSS}</style></head><body><h1><a href=\"/{q}\">ZPOD</a> / {}</h1><div class=\"box\">", esc(&format!("{area}/{relp}")));
    if !relp.is_empty() {
        let up = relp.rsplit_once('/').map(|(a, _)| format!("/files/{area}/{}", opds::href_path(a))).unwrap_or(format!("/files/{area}"));
        h += &format!("<div class=\"r\"><a href=\"{up}{q}\">.. (up)</a></div>");
    }
    if ents.is_empty() {
        h += "<small>(empty)</small>";
    }
    for (d, n, sz) in ents.iter().take(2000) {
        let href = format!("{base}/{}{q}", opds::href_path(n));
        if *d {
            h += &format!("<div class=\"r\"><a href=\"{href}\">{}/</a></div>", esc(n));
        } else {
            h += &format!("<div class=\"r\"><a href=\"{href}\" download>{}</a><small>{}</small></div>", esc(n), human(*sz));
        }
    }
    h + "</div></body></html>"
}

fn store(r: &mut impl Read, root: &Path, rel: &Path, len: u64) -> std::io::Result<()> {
    let dest = root.join(rel);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let part = dest.with_file_name(format!(".{}.part", dest.file_name().unwrap().to_string_lossy()));
    let res = (|| {
        let mut f = std::fs::File::create(&part)?;
        let n = std::io::copy(&mut r.take(len), &mut f)?;
        if n != len {
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, format!("got {n} of {len} bytes")));
        }
        f.sync_all()?;
        std::fs::rename(&part, &dest)
    })();
    if res.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    res
}

fn mpd_update() -> bool {
    zpod_mpd::Client::connect_default().and_then(|mut c| c.simple("update")).is_ok()
}

fn listener_from_env() -> Option<TcpListener> {
    let pid: u32 = std::env::var("LISTEN_PID").ok()?.parse().ok()?;
    let n: i32 = std::env::var("LISTEN_FDS").ok()?.parse().ok()?;
    if pid != std::process::id() || n < 1 {
        return None;
    }
    use std::os::fd::FromRawFd;
    Some(unsafe { TcpListener::from_raw_fd(3) })
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let conf_path = args.iter().position(|a| a == "-c").and_then(|i| args.get(i + 1)).cloned().unwrap_or_else(|| "/etc/zpod/drop.conf".into());
    let mut cfg = parse_config(&std::fs::read_to_string(&conf_path).unwrap_or_default());
    if let Some(i) = args.iter().position(|a| a == "--listen") {
        if let Some(v) = args.get(i + 1) {
            cfg.listen = v.clone();
        }
    }
    if let Some(i) = args.iter().position(|a| a == "--music-dir") {
        if let Some(v) = args.get(i + 1) {
            cfg.music_dir = v.into();
        }
    }
    let activated = listener_from_env();
    let socket_activated = activated.is_some();
    let listener = match activated {
        Some(l) => l,
        None => match TcpListener::bind(&cfg.listen) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("zpod-drop: bind {}: {e}", cfg.listen);
                std::process::exit(1);
            }
        },
    };
    let _ = std::fs::create_dir_all(&cfg.music_dir);
    eprintln!(
        "zpod-drop: on {} drop box {} ({} MB/file), book catalog {} ({}); home Wi-Fi token: {}",
        listener.local_addr().map(|a| a.to_string()).unwrap_or_default(),
        if cfg.dropbox { "ON" } else { "off" },
        cfg.max_upload >> 20,
        if cfg.opds { "ON" } else { "off" },
        cfg.books_dir.display(),
        if cfg.token.is_some() { "set" } else { "none (USB + hotspot only)" }
    );
    let sh = Arc::new(Shared {
        cfg,
        books: Mutex::new(HashMap::new()),
        last_upload: Mutex::new(None),
        last_activity: AtomicU64::new(0),
        active: AtomicUsize::new(0),
        t0: Instant::now(),
    });
    // Debounced MPD rescan 3 s after the last upload; idle exit when socket-activated.
    {
        let sh = sh.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(1));
            let mut lu = sh.last_upload.lock().unwrap();
            if lu.is_some_and(|t| t.elapsed() > Duration::from_secs(3)) && sh.active.load(Ordering::SeqCst) == 0 {
                *lu = None;
                drop(lu);
                eprintln!("zpod-drop: mpd update -> {}", if mpd_update() { "ok" } else { "mpd not reachable" });
                continue;
            }
            drop(lu);
            let idle = sh.t0.elapsed().as_secs() - sh.last_activity.load(Ordering::SeqCst);
            if socket_activated && sh.active.load(Ordering::SeqCst) == 0 && sh.last_upload.lock().unwrap().is_none() && idle > IDLE_EXIT.as_secs() {
                std::process::exit(0); // systemd re-activates on the next connection
            }
        });
    }
    for conn in listener.incoming() {
        let Ok(s) = conn else { continue };
        sh.last_activity.store(sh.t0.elapsed().as_secs(), Ordering::SeqCst);
        if sh.active.load(Ordering::SeqCst) >= MAX_CONN {
            let mut s = s;
            respond(&mut s, 500, "text/plain", b"busy\n");
            continue;
        }
        sh.active.fetch_add(1, Ordering::SeqCst);
        let sh2 = sh.clone();
        std::thread::spawn(move || {
            handle(s, &sh2);
            sh2.last_activity.store(sh2.t0.elapsed().as_secs(), Ordering::SeqCst);
            sh2.active.fetch_sub(1, Ordering::SeqCst);
        });
    }
}

const CSS: &str = "body{font:15px -apple-system,system-ui,sans-serif;background:#080a18;color:#ececf0;max-width:680px;margin:1.5em auto;padding:0 1em}h1{color:#ffc400;margin:0 0 .3em;font-size:1.4em}h1 a{color:#ffc400;text-decoration:none}a{color:#40c8ff}.box{background:#161a34;border-radius:10px;padding:1em;margin:1em 0}.r{display:flex;justify-content:space-between;gap:1em;padding:.25em 0;border-bottom:1px solid #22264a}small{color:#8c90a8}button,label.b{background:#0060b0;color:#fff;border:0;border-radius:6px;padding:.5em 1em;cursor:pointer;display:inline-block;margin:.2em 0}input[type=file]{display:none}progress{width:100%}#log{font:12px ui-monospace,monospace;white-space:pre-wrap;max-height:40vh;overflow:auto;color:#8c90a8}code{color:#ffc400}.hidden{display:none}";

const PAGE: &str = r#"<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>ZPOD drop box</title><style>CSSHERE</style></head><body>
<h1>ZPOD drop box</h1><small id="st"></small>
<div class="box"><b>Browse &amp; download</b><div class="r"><a id="lm" href="files/music">Music</a><a id="lb" href="files/books">Books</a><a id="li" href="files/inbox">Inbox</a></div></div>
<div class="box"><b>Upload to the inbox</b> <small>(/data/inbox, up to {{MAX}} MB per file)</small><br>
<label class="b">Choose files<input id="files" type="file" multiple></label>
<p><progress id="p" value="0" max="1"></progress></p><div id="log"></div></div>
<div class="box"><b>Add music</b> <small>(keeps Artist/Album folders; the library rescans itself)</small><br>
<label class="b">Album folder<input id="dir" type="file" webkitdirectory multiple></label>
<label class="b">Music files<input id="mf" type="file" multiple></label></div>
<div class="box {{OPDS}}"><b>Book catalog (OPDS)</b><br>In KOReader, Thorium, Marvin...: add catalog <code id="ou">/opds</code></div>
<script>
const tok=new URLSearchParams(location.search).get('token');const H=tok?{'X-Zpod-Token':tok}:{};const Q=tok?'?token='+encodeURIComponent(tok):'';
for(const id of['lm','lb','li'])document.getElementById(id).href+=Q;document.getElementById('ou').textContent=location.origin+'/opds'+Q;
const log=t=>{const l=document.getElementById('log');l.textContent+=t+'\n';l.scrollTop=1e9};
fetch('status',{headers:H}).then(r=>r.json()).then(j=>{document.getElementById('st').textContent=(j.free_bytes/1e9).toFixed(1)+' GB free'}).catch(()=>{});
const mus=/\.(mp3|flac|ogg|oga|opus|m4a|aac|wav|aiff?|wv|ape|mpc|jpe?g|png|m3u8?|cue)$/i;
async function up(list,music){const fs=[...list].filter(f=>!f.name.startsWith('.')&&(!music||mus.test(f.name)));
const tot=fs.reduce((a,f)=>a+f.size,0)||1;let done=0;const p=document.getElementById('p');
for(const f of fs){const rel=(f.webkitRelativePath||f.name).normalize('NFC');
const url=music?'music/'+rel.split('/').map(encodeURIComponent).join('/'):'inbox/'+encodeURIComponent(f.name.normalize('NFC'));
if(!music&&f.size>{{MAX}}*1048576){log('SKIP '+rel+' (over {{MAX}} MB)');done+=f.size;continue}
try{const r=await fetch(url,{method:'PUT',body:f,headers:H});log((r.ok?'ok   ':'FAIL ')+rel+' '+(await r.text()).trim())}
catch(e){log('FAIL '+rel+' '+e)}done+=f.size;p.value=done/tot}
log('done: '+fs.length+' files')}
document.getElementById('files').onchange=e=>up(e.target.files,false);
document.getElementById('dir').onchange=e=>up(e.target.files,true);document.getElementById('mf').onchange=e=>up(e.target.files,true);
</script></body></html>"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths() {
        assert_eq!(safe_music_path("Nova%20Echo/Night%20Drive/01%20Neon.flac").unwrap(), PathBuf::from("Nova Echo/Night Drive/01 Neon.flac"));
        assert_eq!(safe_music_path("Sigur%20Fj%C3%B6rd/a.mp3").unwrap(), PathBuf::from("Sigur Fjörd/a.mp3"));
        assert!(safe_music_path("../etc/passwd.mp3").is_err());
        assert!(safe_music_path("a/%2e%2e/%2e%2e/x.mp3").is_err());
        assert!(safe_music_path("/abs.mp3").is_err());
        assert!(safe_music_path(".hidden/x.mp3").is_err());
        assert!(safe_music_path("evil.sh").is_err());
        assert!(safe_music_path("noext").is_err());
        assert!(safe_music_path("bad%zz.mp3").is_err());
        assert!(safe_music_path("trunc%4").is_err());
    }

    #[test]
    fn nets_and_config() {
        let usb = (Ipv4Addr::new(10, 12, 194, 0), 28);
        assert!(in_net("10.12.194.2".parse().unwrap(), usb));
        assert!(in_net("10.12.194.14".parse().unwrap(), usb));
        assert!(!in_net("10.12.194.17".parse().unwrap(), usb));
        assert!(!in_net("192.168.1.20".parse().unwrap(), usb));
        let c = parse_config("listen = 0.0.0.0:9000\ntoken = short\nmusic_dir=/x # c\n");
        assert_eq!(c.listen, "0.0.0.0:9000");
        assert_eq!(c.token, None, "tokens under 8 chars are ignored");
        assert_eq!(c.music_dir, PathBuf::from("/x"));
        assert!(parse_config("token = 0123456789abcdef").token.is_some());
    }

    #[test]
    fn m6_paths_names_auth() {
        assert_eq!(safe_rel("").unwrap(), PathBuf::new());
        assert_eq!(safe_rel("Sci%20Fi/a.epub").unwrap(), PathBuf::from("Sci Fi/a.epub"));
        assert!(safe_rel("../x").is_err() && safe_rel("a/.git/x").is_err() && safe_rel("/etc").is_err());
        assert_eq!(safe_inbox_name("My%20Notes.pdf").unwrap(), "My Notes.pdf");
        for bad in ["", ".env", "a%2Fb", "..", "a%00b", "x%0A"] {
            assert!(safe_inbox_name(bad).is_err(), "{bad}");
        }
        assert_eq!(b64_decode("enBvZDpzZWNyZXQxMjM=").unwrap(), b"zpod:secret123");
        assert_eq!(b64_decode("YQ").unwrap(), b"a");
        let c = parse_config("dropbox = on\nopds=yes\nmax_upload_mb = 100\n");
        assert!(c.dropbox && c.opds);
        assert_eq!(c.max_upload, 100 << 20);
        let d = Config::default();
        assert!(!d.dropbox && !d.opds, "both off by default");
        assert!(in_net("10.42.0.23".parse().unwrap(), d.hotspot_net));
        assert!(!in_net("10.42.1.2".parse().unwrap(), d.hotspot_net));
        assert_eq!(content_type(Path::new("x.html")), "text/plain; charset=utf-8", "uploaded HTML never renders");
        let dir = std::env::temp_dir().join(format!("zuniq-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), b"1").unwrap();
        assert_eq!(unique(&dir, "a.txt"), dir.join("a (2).txt"));
        assert_eq!(unique(&dir, "b.txt"), dir.join("b.txt"));
        let html = listing("inbox", Path::new(""), &dir, "");
        assert!(html.contains("href=\"/files/inbox/a.txt\" download>a.txt<"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn store_is_atomic() {
        let d = std::env::temp_dir().join(format!("zdrop-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let rel = PathBuf::from("A/B/t.flac");
        store(&mut &b"hello"[..], &d, &rel, 5).unwrap();
        assert_eq!(std::fs::read(d.join(&rel)).unwrap(), b"hello");
        assert!(store(&mut &b"hi"[..], &d, Path::new("A/B/u.flac"), 5).is_err());
        assert!(!d.join("A/B/u.flac").exists() && !d.join("A/B/.u.flac.part").exists());
        std::fs::remove_dir_all(&d).unwrap();
    }
}
