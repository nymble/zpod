//! Tools > Drop box / Book catalog (M6): one switch each for the two halves
//! of zpod-drop. Both OFF by default. The socket (port 8080) is enabled
//! when either is on and disabled when both are off; zpod-drop reads
//! `dropbox =` / `opds =` from /etc/zpod/drop.conf at start, so a change
//! restarts the service.

use std::process::{Command, Stdio};

use zpod_hal::sys;

pub const PORT: u16 = 8080;
const SOCKET: &str = "zpod-drop.socket";
const SERVICE: &str = "zpod-drop.service";

pub fn conf_path() -> std::path::PathBuf {
    std::env::var_os("ZPOD_DROP_CONF").map(Into::into).unwrap_or_else(|| "/etc/zpod/drop.conf".into())
}

/// Value of `key = v` in a flat key=value file.
pub fn get_kv(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|l| {
        let l = l.split('#').next()?.trim();
        let (k, v) = l.split_once('=')?;
        (k.trim() == key).then(|| v.trim().trim_matches('"').to_string())
    })
}

/// Set `key = value`: replaces the first active line (later duplicates
/// dropped), else appends. Comments stay as they are.
pub fn set_kv(text: &str, key: &str, value: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut done = false;
    for line in text.lines() {
        let active = !line.trim_start().starts_with('#') && line.split_once('=').is_some_and(|(k, _)| k.trim() == key);
        if active {
            if !done {
                out.push(format!("{key} = {value}"));
                done = true;
            }
        } else {
            out.push(line.to_string());
        }
    }
    if !done {
        out.push(format!("{key} = {value}"));
    }
    out.join("\n") + "\n"
}

fn is_on(v: Option<String>) -> bool {
    v.is_some_and(|v| matches!(v.to_ascii_lowercase().as_str(), "on" | "yes" | "true" | "1"))
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    pub dropbox: bool,
    pub opds: bool,
    pub token: bool,
    pub max_mb: u64,
    pub running: bool,
    pub books: usize,
    pub inbox: usize,
}

fn count_files(dir: &str, depth: u32) -> usize {
    let Ok(rd) = std::fs::read_dir(dir) else { return 0 };
    rd.flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() && depth < 4 => count_files(&e.path().to_string_lossy(), depth + 1),
            Ok(t) if t.is_file() && !e.file_name().to_string_lossy().starts_with('.') => 1,
            _ => 0,
        })
        .sum()
}

pub fn state() -> State {
    let t = std::fs::read_to_string(conf_path()).unwrap_or_default();
    let sim = sys::sim();
    State {
        dropbox: is_on(get_kv(&t, "dropbox")),
        opds: is_on(get_kv(&t, "opds")),
        token: get_kv(&t, "token").is_some_and(|v| v.len() >= 8),
        max_mb: get_kv(&t, "max_upload_mb").and_then(|v| v.parse().ok()).unwrap_or(1024),
        running: if sim { is_on(get_kv(&t, "dropbox")) || is_on(get_kv(&t, "opds")) } else { listening() },
        books: if sim { 42 } else { count_files("/data/books", 0) },
        inbox: if sim { 3 } else { count_files("/data/inbox", 0) },
    }
}

fn listening() -> bool {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], PORT));
    std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(150)).is_ok()
}

fn systemctl(args: &[&str]) -> Result<(), String> {
    let o = Command::new("systemctl").args(args).stdout(Stdio::null()).output().map_err(|e| e.to_string())?;
    if o.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&o.stderr).lines().last().unwrap_or("systemctl failed").to_string())
    }
}

/// Flip one half (opds = true: book catalog; false: drop box).
pub fn toggle(opds: bool) -> Result<State, String> {
    let p = conf_path();
    let t = std::fs::read_to_string(&p).unwrap_or_default();
    let key = if opds { "opds" } else { "dropbox" };
    let now = !is_on(get_kv(&t, key));
    let t2 = set_kv(&t, key, if now { "on" } else { "off" });
    let tmp = p.with_extension("tmp");
    std::fs::write(&tmp, &t2).and_then(|_| std::fs::rename(&tmp, &p)).map_err(|e| format!("{}: {e}", p.display()))?;
    if !sys::sim() {
        let any = is_on(get_kv(&t2, "dropbox")) || is_on(get_kv(&t2, "opds"));
        if any {
            systemctl(&["enable", "--now", SOCKET])?;
            // pick up the new switches (the socket keeps listening)
            let _ = systemctl(&["try-restart", SERVICE]);
        } else {
            systemctl(&["disable", "--now", SOCKET])?;
            let _ = systemctl(&["stop", SERVICE]);
        }
    }
    Ok(state())
}

/// The addresses to show: (label, url, note).
pub fn urls(opds: bool, hotspot_on: bool) -> Vec<(&'static str, String, &'static str)> {
    let path = if opds { "/opds" } else { "/" };
    let mut v = Vec::new();
    v.push(("USB", format!("http://{}:{PORT}{path}", sys::ipv4("usb0").unwrap_or_else(|| "10.12.194.1".into())), ""));
    if hotspot_on {
        v.push(("Hotspot", format!("http://{}:{PORT}{path}", zpod_hal::wifi::HOTSPOT_IP), ""));
    } else if let Some(ip) = sys::ipv4("wlan0") {
        v.push(("Wi-Fi", format!("http://{ip}:{PORT}{path}"), "needs token"));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kv_edit() {
        let t = "# comment\nmusic_dir = /srv/zpod/media/music\n# token =\n";
        let a = set_kv(t, "dropbox", "on");
        assert_eq!(get_kv(&a, "dropbox").as_deref(), Some("on"));
        let b = set_kv(&a, "dropbox", "off");
        assert_eq!(get_kv(&b, "dropbox").as_deref(), Some("off"));
        assert_eq!(b.matches("dropbox").count(), 1);
        assert!(b.contains("# token =\n"), "comments kept");
        assert_eq!(get_kv(&b, "token"), None);
        assert!(!is_on(get_kv(t, "opds")));
    }

    #[test]
    fn toggle_writes_conf() {
        let d = std::env::temp_dir().join(format!("zsrv-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("drop.conf");
        std::fs::write(&p, "music_dir = /x\n").unwrap();
        std::env::set_var("ZPOD_DROP_CONF", &p);
        std::env::set_var("ZPOD_SIM", &d);
        let s = toggle(true).unwrap();
        assert!(s.opds && !s.dropbox && s.running);
        let s = toggle(true).unwrap();
        assert!(!s.opds && !s.running);
        std::fs::remove_dir_all(&d).unwrap();
    }
}
