//! File transfer screen data (M5): how to connect, free space, the newest
//! files in the music library, MPD rescan state, and the optional rsync
//! share. OK = rescan the library now, Square = rsync share on/off.

use std::process::{Command, Stdio};

use zpod_hal::{storage, sys};

pub const MEDIA: &str = "/srv/zpod/media";
pub const RSYNC_UNIT: &str = "zpod-rsyncd.socket";

#[derive(Clone, Debug, Default)]
pub struct Info {
    pub usb: Option<String>,
    pub wlan: Option<String>,
    pub host: String,
    pub free: Option<storage::FsUsage>,
    pub on_data: bool,
    /// Newest files (path relative to the music folder).
    pub recent: Vec<String>,
    pub songs: Option<u64>,
    pub db_update: Option<i64>,
    pub updating: bool,
    pub mpd_ok: bool,
    pub drop_on: bool,
    pub rsync: Option<bool>,
}

/// `systemctl is-enabled`/`is-active` for the optional rsync daemon socket.
/// None = not installed.
pub fn rsync_state() -> Option<bool> {
    if sys::sim() {
        return Some(std::env::var_os("ZPOD_SIM_RSYNC").is_some());
    }
    if !std::path::Path::new("/etc/systemd/system").join(RSYNC_UNIT).exists() && !std::path::Path::new("/usr/lib/systemd/system").join(RSYNC_UNIT).exists() {
        return None;
    }
    let st = Command::new("systemctl").args(["is-active", "--quiet", RSYNC_UNIT]).stdout(Stdio::null()).stderr(Stdio::null()).status();
    Some(st.map(|s| s.success()).unwrap_or(false))
}

/// Turn the rsync share on (enable --now) or off (disable --now). Root only.
pub fn set_rsync(on: bool) -> Result<(), String> {
    if sys::sim() {
        return Ok(());
    }
    let o = Command::new("systemctl")
        .args([if on { "enable" } else { "disable" }, "--now", RSYNC_UNIT])
        .stdout(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    if o.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&o.stderr).lines().last().unwrap_or("systemctl failed").to_string())
    }
}

/// Parse `find ... sort -Last-Modified window 0:N` into file paths.
pub fn recent_files(pairs: &[(String, String)]) -> Vec<String> {
    pairs.iter().filter(|(k, _)| k == "file").map(|(_, v)| v.clone()).collect()
}

/// Ask MPD to rescan the library. Returns the job id text.
pub fn rescan() -> Result<(), String> {
    if sys::sim() {
        return Ok(());
    }
    let mut c = zpod_mpd::Client::connect_default().map_err(|e| format!("MPD: {e:?}"))?;
    c.cmd("update").map(|_| ()).map_err(|e| format!("MPD: {e:?}"))
}

pub fn gather(drop_on: bool) -> Info {
    let mut i = Info {
        usb: sys::ipv4("usb0"),
        wlan: sys::ipv4("wlan0"),
        host: std::fs::read_to_string("/proc/sys/kernel/hostname").map(|h| h.trim().to_string()).unwrap_or_else(|_| "zpod".into()),
        free: storage::fs_usage(MEDIA),
        drop_on,
        rsync: rsync_state(),
        ..Default::default()
    };
    if let (Some(m), Some(d)) = (storage::fs_usage(MEDIA), storage::fs_usage(storage::DATA)) {
        i.on_data = m.dev == d.dev;
    }
    if sys::sim() {
        i.mpd_ok = true;
        i.songs = Some(1234);
        i.db_update = Some(sys::unix_now() - 600);
        i.recent = vec!["Incoming/Some Artist - New Song.mp3".into(), "Artist/Album (2024)/01 Opening.flac".into(), "Artist/Album (2024)/02 Second.flac".into()];
        return i;
    }
    if let Ok(mut c) = zpod_mpd::Client::connect_default() {
        i.mpd_ok = true;
        if let Ok(st) = c.status() {
            i.updating = st.updating_db;
        }
        if let Ok(p) = c.cmd("stats") {
            for (k, v) in &p {
                match k.as_str() {
                    "songs" => i.songs = v.parse().ok(),
                    "db_update" => i.db_update = v.parse().ok(),
                    _ => {}
                }
            }
        }
        if let Ok(p) = c.cmd("find \"(modified-since '1')\" sort -Last-Modified window 0:4") {
            i.recent = recent_files(&p);
        }
    }
    i
}

/// "14:02" today, else "Oct 27".
pub fn when(unix: i64, now: &zpod_core::DateTime) -> String {
    // local offset from now_local vs unix_now
    let off = now.to_unix() - sys::unix_now();
    let t = zpod_core::DateTime::from_unix(unix + off);
    if (t.year, t.month, t.day) == (now.year, now.month, now.day) {
        format!("{:02}:{:02}", t.hour, t.minute)
    } else {
        const M: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
        format!("{} {}", M[(t.month.clamp(1, 12) - 1) as usize], t.day)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_recent() {
        let p = vec![
            ("file".to_string(), "A/B/01.flac".to_string()),
            ("Last-Modified".to_string(), "2026-10-29T10:00:00Z".to_string()),
            ("Title".to_string(), "x".to_string()),
            ("file".to_string(), "C.mp3".to_string()),
        ];
        assert_eq!(recent_files(&p), ["A/B/01.flac", "C.mp3"]);
    }
}
