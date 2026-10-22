//! zpod-drop: add music to the ZPOD from any browser or `curl -T`.
//!
//!   GET  /                 upload page (pick files or a whole folder)
//!   PUT  /music/<path>     store one file under the music dir (atomic: .part + rename)
//!   POST /update           ask MPD to rescan (also done automatically after uploads)
//!   GET  /status           JSON: free bytes, music dir
//!
//! Access: peers on the USB gadget subnet (10.12.194.0/28) are trusted; any
//! other peer (Wi-Fi) must send the token from /etc/zpod/drop.conf as header
//! `X-Zpod-Token` or `?token=`. No token configured => USB only.
//! Socket activation: if LISTEN_FDS=1 (systemd .socket), serve fd 3 and exit
//! after 10 idle minutes; otherwise bind `listen` from the config.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, Ipv4Addr, TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const MAX_FILE: u64 = 2 << 30;
const MAX_CONN: usize = 4;
const IDLE_EXIT: Duration = Duration::from_secs(600);

#[derive(Clone, Debug)]
struct Config {
    listen: String,
    music_dir: PathBuf,
    token: Option<String>,
    usb_net: (Ipv4Addr, u8),
}

impl Default for Config {
    fn default() -> Self {
        Config {
            listen: "0.0.0.0:8080".into(),
            music_dir: "/srv/zpod/media/music".into(),
            token: None,
            usb_net: (Ipv4Addr::new(10, 12, 194, 0), 28),
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
            "token" if v.len() >= 8 => c.token = Some(v),
            _ => {}
        }
    }
    c
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

fn pct_decode(s: &str) -> Option<String> {
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
    let reason = match code {
        200 => "OK",
        201 => "Created",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        411 => "Length Required",
        413 => "Payload Too Large",
        500 => "Internal Server Error",
        507 => "Insufficient Storage",
        _ => "",
    };
    let _ = write!(s, "HTTP/1.1 {code} {reason}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n", body.len());
    let _ = s.write_all(body);
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
    let trusted = in_net(peer, sh.cfg.usb_net) || peer.is_loopback();
    let token_ok = sh.cfg.token.as_deref().is_some_and(|t| {
        req.header("X-Zpod-Token") == Some(t) || req.query_param("token").as_deref() == Some(t)
    });
    if !trusted && !token_ok {
        let msg = if sh.cfg.token.is_some() {
            "403: token required for Wi-Fi uploads (see /etc/zpod/drop.conf)\n"
        } else {
            "403: uploads over Wi-Fi are disabled; use the USB cable, or set token= in /etc/zpod/drop.conf\n"
        };
        respond(&mut s, 403, "text/plain", msg.as_bytes());
        return;
    }
    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/") => respond(&mut s, 200, "text/html; charset=utf-8", PAGE.as_bytes()),
        ("GET", "/status") => {
            let body = format!(
                "{{\"free_bytes\":{},\"music_dir\":\"{}\",\"core\":\"{}\"}}\n",
                free_bytes(&sh.cfg.music_dir),
                sh.cfg.music_dir.display(),
                env!("CARGO_PKG_VERSION")
            );
            respond(&mut s, 200, "application/json", body.as_bytes());
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
            if len + (64 << 20) > free_bytes(&sh.cfg.music_dir) {
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
        "zpod-drop: serving {} on {} (wifi token: {})",
        cfg.music_dir.display(),
        listener.local_addr().map(|a| a.to_string()).unwrap_or_default(),
        if cfg.token.is_some() { "set" } else { "none, USB only" }
    );
    let sh = Arc::new(Shared {
        cfg,
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

const PAGE: &str = r#"<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>ZPOD drop</title><style>
body{font:15px -apple-system,system-ui,sans-serif;background:#080a18;color:#ececf0;max-width:640px;margin:2em auto;padding:0 1em}
h1{color:#ffc400;margin:0 0 .2em}.box{background:#161a34;border-radius:10px;padding:1em;margin:1em 0}
button,label.b{background:#0060b0;color:#fff;border:0;border-radius:6px;padding:.5em 1em;cursor:pointer;display:inline-block}
#log{font:12px ui-monospace,monospace;white-space:pre-wrap;max-height:40vh;overflow:auto;color:#8c90a8}
progress{width:100%}small{color:#8c90a8}input[type=file]{display:none}</style></head><body>
<h1>ZPOD drop</h1><small id="st">music folder</small>
<div class="box"><p>Pick an album folder (keeps <i>Artist/Album/</i> structure) or single files.</p>
<label class="b">Folder<input id="dir" type="file" webkitdirectory multiple></label>
<label class="b">Files<input id="files" type="file" multiple></label>
<p><progress id="p" value="0" max="1"></progress></p><div id="log"></div></div>
<small>Tip: from a Mac terminal, <code>scripts/zpod-push-music.sh</code> (rsync) is faster for big libraries.</small>
<script>
const tok=new URLSearchParams(location.search).get('token');const H=tok?{'X-Zpod-Token':tok}:{};
const log=t=>{const l=document.getElementById('log');l.textContent+=t+'\n';l.scrollTop=1e9};
fetch('status',{headers:H}).then(r=>r.json()).then(j=>{document.getElementById('st').textContent=j.music_dir+' \u00b7 '+(j.free_bytes/1e9).toFixed(1)+' GB free'}).catch(()=>{});
const ok=/\.(mp3|flac|ogg|oga|opus|m4a|aac|wav|aiff?|wv|ape|mpc|jpe?g|png|m3u8?|cue)$/i;
async function up(list){const fs=[...list].filter(f=>ok.test(f.name)&&!f.name.startsWith('.'));
const tot=fs.reduce((a,f)=>a+f.size,0)||1;let done=0;const p=document.getElementById('p');
for(const f of fs){const rel=(f.webkitRelativePath||f.name).normalize('NFC');
const url='music/'+rel.split('/').map(encodeURIComponent).join('/');
try{const r=await fetch(url,{method:'PUT',body:f,headers:H});log((r.ok?'ok   ':'FAIL ')+rel+(r.ok?'':' '+(await r.text()).trim()))}
catch(e){log('FAIL '+rel+' '+e)}done+=f.size;p.value=done/tot}
log('done: '+fs.length+' files; the library rescans automatically')}
document.getElementById('dir').onchange=e=>up(e.target.files);document.getElementById('files').onchange=e=>up(e.target.files);
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
