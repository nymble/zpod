//! System status helpers (no subprocess except amixer/nmcli/iw).

use std::ffi::CStr;
use std::time::{Duration, Instant};

use zpod_core::{DateTime, PlayState, StatusInfo};
use zpod_drivers::max17048::Max17048;

pub const CARD: &str = "sndrpiaoidezpod";
pub const MIXER: &str = "Digital";

pub fn sim() -> bool {
    std::env::var_os("ZPOD_SIM").is_some()
}

fn env_f32(k: &str) -> Option<f32> {
    std::env::var(k).ok()?.parse().ok()
}

static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

pub fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Local wall-clock time. `ZPOD_FAKE_TIME="2026-10-17 10:08:42"` pins the
/// start (screenshots/tests); it then ticks in real time.
pub fn now_local() -> DateTime {
    let start = *START.get_or_init(Instant::now);
    if let Some(t) = std::env::var("ZPOD_FAKE_TIME").ok().and_then(|s| DateTime::parse(&s)) {
        return DateTime::from_unix(t.to_unix() + start.elapsed().as_secs() as i64);
    }
    // libc::time_t is deprecated on musl (planned move to 64-bit); the libc
    // crate's localtime_r binding uses the same width, so this is consistent.
    #[allow(deprecated)]
    let now = unix_now() as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&now, &mut tm) }.is_null() {
        return DateTime::from_unix(now as i64);
    }
    DateTime {
        year: tm.tm_year + 1900,
        month: (tm.tm_mon + 1) as u8,
        day: tm.tm_mday as u8,
        hour: tm.tm_hour as u8,
        minute: tm.tm_min as u8,
        second: tm.tm_sec as u8,
        weekday: tm.tm_wday as u8,
    }
}

/// Sub-second part of the clock (for aligning redraws to the second edge).
pub fn subsec_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_millis() as u64)
        .unwrap_or(0)
}

/// IPv4 address of an interface (getifaddrs, no subprocess).
pub fn ipv4(ifname: &str) -> Option<String> {
    if sim() {
        let k = format!("ZPOD_SIM_IP_{}", ifname.to_uppercase());
        return std::env::var(k).ok();
    }
    let mut ifa: *mut libc::ifaddrs = std::ptr::null_mut();
    if unsafe { libc::getifaddrs(&mut ifa) } != 0 {
        return None;
    }
    let mut cur = ifa;
    let mut out = None;
    while !cur.is_null() {
        let a = unsafe { &*cur };
        if !a.ifa_addr.is_null() && unsafe { (*a.ifa_addr).sa_family } as i32 == libc::AF_INET {
            let name = unsafe { CStr::from_ptr(a.ifa_name) }.to_string_lossy();
            if name == ifname {
                let sin = unsafe { &*(a.ifa_addr as *const libc::sockaddr_in) };
                let ip = std::net::Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr));
                out = Some(ip.to_string());
                break;
            }
        }
        cur = a.ifa_next;
    }
    unsafe { libc::freeifaddrs(ifa) };
    out
}

fn run(args: &[&str]) -> String {
    std::process::Command::new(args[0])
        .args(&args[1..])
        .stderr(std::process::Stdio::null())
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

pub fn parse_amixer_pct(out: &str) -> Option<u8> {
    let i = out.find('[')?;
    let rest = &out[i + 1..];
    let j = rest.find("%]")?;
    rest[..j].parse().ok()
}

/// Volume state published by zpod-buttons in /run/zpod/volume:
/// (percent, a volume key is held). None if the daemon has not written it.
pub fn volume_state() -> Option<(u8, bool)> {
    if sim() {
        return env_f32("ZPOD_SIM_VOL").map(|v| (v as u8, false));
    }
    let p = std::env::var("ZPOD_VOLUME_FILE").unwrap_or_else(|_| zpod_core::volume::STATE_FILE.into());
    zpod_core::volume::parse_state(&std::fs::read_to_string(p).ok()?)
}

/// ALSA 'Digital' volume in percent: the daemon's state file (cheap, no
/// subprocess), else one amixer read (daemon not running yet).
/// Apps never *set* the volume: zpod-buttons is the only writer.
pub fn volume_get() -> Option<u8> {
    if let Some((v, _)) = volume_state() {
        return Some(v);
    }
    if sim() {
        return None;
    }
    parse_amixer_pct(&run(&["amixer", "-c", CARD, "sget", MIXER]))
}

pub fn card_present() -> bool {
    std::fs::read_to_string("/proc/asound/cards").map(|s| s.contains(&format!("[{CARD}"))).unwrap_or(false)
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct WifiInfo {
    pub ssid: Option<String>,
    pub signal: Option<String>,
    pub ip: Option<String>,
    pub powersave: Option<String>,
    /// wlan0 hardware (MAC) address.
    pub mac: Option<String>,
}

/// MAC address of an interface from sysfs.
pub fn mac_address(ifname: &str) -> Option<String> {
    if sim() {
        return std::env::var(format!("ZPOD_SIM_MAC_{}", ifname.to_uppercase())).ok();
    }
    let a = std::fs::read_to_string(format!("/sys/class/net/{ifname}/address")).ok()?;
    let a = a.trim();
    (!a.is_empty()).then(|| a.to_string())
}

pub fn wifi_info() -> WifiInfo {
    let mut w = WifiInfo { ip: ipv4("wlan0"), mac: mac_address("wlan0"), ..Default::default() };
    if sim() {
        w.ssid = std::env::var("ZPOD_SIM_SSID").ok();
        w.signal = Some("72".into());
        w.powersave = Some("off".into());
        return w;
    }
    for line in run(&["nmcli", "-t", "-f", "ACTIVE,SSID,SIGNAL", "device", "wifi", "list", "--rescan", "no"]).lines() {
        let parts: Vec<&str> = line.split(':').collect();
        if parts.len() >= 3 && parts[0] == "yes" {
            w.ssid = Some(parts[1..parts.len() - 1].join(":"));
            w.signal = Some(parts[parts.len() - 1].to_string());
            break;
        }
    }
    if w.ssid.is_none() {
        // No NetworkManager (Buildroot image): `iw dev wlan0 link` gives the
        // SSID and the signal in dBm (mapped to a 0-100 % like nmcli's).
        let link = run(&["iw", "dev", "wlan0", "link"]);
        for l in link.lines().map(str::trim) {
            if let Some(v) = l.strip_prefix("SSID: ") {
                w.ssid = Some(v.to_string());
            } else if let Some(v) = l.strip_prefix("signal: ") {
                if let Some(dbm) = v.split_whitespace().next().and_then(|x| x.parse::<i32>().ok()) {
                    w.signal = Some((2 * (dbm + 100)).clamp(0, 100).to_string());
                }
            }
        }
    }
    let ps = run(&["iw", "dev", "wlan0", "get", "power_save"]);
    if let Some(i) = ps.find("Power save:") {
        w.powersave = ps[i + 11..].split_whitespace().next().map(|s| s.to_string());
    }
    w
}

pub fn uptime() -> String {
    let s = std::fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|t| t.split_whitespace().next().and_then(|x| x.parse::<f64>().ok()))
        .unwrap_or(0.0) as u64;
    let (d, h, m) = (s / 86400, s / 3600 % 24, s / 60 % 60);
    if d > 0 { format!("{d}d {h}h{m:02}m") } else { format!("{h}h{m:02}m") }
}

/// MAX17048 over /dev/i2c-1 through the embedded-hal driver.
pub struct Gauge {
    dev: Option<Max17048<linux_embedded_hal::I2cdev>>,
    next_try: Instant,
}

impl Default for Gauge {
    fn default() -> Self {
        Gauge { dev: None, next_try: Instant::now() }
    }
}

impl Gauge {
    /// (percent, volts, charging)
    pub fn read(&mut self) -> Option<(f32, f32, bool)> {
        if sim() {
            return env_f32("ZPOD_SIM_BATT").map(|p| (p, 3.9, false));
        }
        if self.dev.is_none() {
            if Instant::now() < self.next_try {
                return None;
            }
            self.next_try = Instant::now() + Duration::from_secs(30);
            self.dev = linux_embedded_hal::I2cdev::new("/dev/i2c-1").ok().map(Max17048::new);
        }
        let g = self.dev.as_mut()?;
        match (g.soc_percent(), g.vcell()) {
            (Ok(p), Ok(v)) => {
                let charging = g.crate_pct_per_hour().map(|r| r > 0.5).unwrap_or(false);
                Some((p, v, charging))
            }
            _ => {
                self.dev = None;
                None
            }
        }
    }
}

/// Status bar data with per-source refresh intervals (keeps CPU low).
pub struct StatusSource {
    gauge: Gauge,
    info: StatusInfo,
    batt_at: Option<Instant>,
    net_at: Option<Instant>,
    vol_at: Option<Instant>,
    mpd_at: Option<Instant>,
    pub query_mpd: bool,
}

impl Default for StatusSource {
    fn default() -> Self {
        Self::new()
    }
}

fn due(t: &mut Option<Instant>, every: Duration) -> bool {
    let now = Instant::now();
    if t.is_none_or(|t| now >= t) {
        *t = Some(now + every);
        true
    } else {
        false
    }
}

impl StatusSource {
    pub fn new() -> Self {
        StatusSource {
            gauge: Gauge::default(),
            info: StatusInfo::default(),
            batt_at: None,
            net_at: None,
            vol_at: None,
            mpd_at: None,
            query_mpd: true,
        }
    }

    /// Force a volume re-read on the next poll (after a volume key).
    pub fn volume_changed(&mut self) {
        self.vol_at = None;
    }

    pub fn set_play(&mut self, p: PlayState) {
        self.info.play = p;
        self.mpd_at = Some(Instant::now() + Duration::from_secs(5));
    }

    pub fn set_volume(&mut self, v: Option<u8>) {
        self.info.volume_pct = v;
        self.vol_at = Some(Instant::now() + Duration::from_secs(10));
    }

    /// Current status (refreshing whatever is due). Cheap to call often.
    pub fn poll(&mut self) -> StatusInfo {
        let t = now_local();
        self.info.time = Some(DateTime { second: 0, ..t });
        if due(&mut self.batt_at, Duration::from_secs(20)) {
            let b = self.gauge.read();
            self.info.battery_pct = b.map(|x| x.0);
            self.info.charging = b.map(|x| x.2).unwrap_or(false);
        }
        if due(&mut self.net_at, Duration::from_secs(5)) {
            self.info.wifi = ipv4("wlan0").is_some();
            self.info.usb = ipv4("usb0").is_some();
        }
        if due(&mut self.vol_at, Duration::from_secs(10)) {
            self.info.volume_pct = volume_get();
        }
        if self.query_mpd && due(&mut self.mpd_at, Duration::from_secs(5)) {
            self.info.play = mpd_play_state();
        }
        self.info.clone()
    }
}

/// Global Play/Pause: toggle MPD. Returns a short message for a toast.
/// Never plays a test tone (that lives in Settings > Diagnostics).
pub fn mpd_toggle() -> (PlayState, &'static str) {
    if sim() && std::env::var_os("MPD_HOST").is_none() {
        return match std::env::var("ZPOD_SIM_PLAY").as_deref() {
            Ok("play") => (PlayState::Paused, "Paused"),
            Ok("pause") => (PlayState::Playing, "Playing"),
            _ => (PlayState::Stopped, "Nothing queued: open Music"),
        };
    }
    let Ok(mut c) = zpod_mpd::Client::connect_default() else {
        return (PlayState::Unknown, "Music service not running");
    };
    let Ok(s) = c.status() else {
        return (PlayState::Unknown, "Music service not running");
    };
    if s.playlist_len == 0 {
        return (PlayState::Stopped, "Nothing queued: open Music");
    }
    let (cmd, st, msg) = match s.state.as_str() {
        "play" => ("pause 1", PlayState::Paused, "Paused"),
        "pause" => ("pause 0", PlayState::Playing, "Playing"),
        _ => ("play", PlayState::Playing, "Playing"),
    };
    let _ = c.simple(cmd);
    (st, msg)
}

/// RTC: (name, "YYYY-MM-DD HH:MM:SS" UTC) of /dev/rtc0, if the DS3231 is bound.
pub fn rtc_info() -> Option<(String, String)> {
    if sim() {
        return std::env::var("ZPOD_SIM_RTC").ok().map(|t| ("ds3231".into(), t));
    }
    let b = "/sys/class/rtc/rtc0";
    let name = std::fs::read_to_string(format!("{b}/name")).ok()?.trim().to_string();
    let d = std::fs::read_to_string(format!("{b}/date")).unwrap_or_default();
    let t = std::fs::read_to_string(format!("{b}/time")).unwrap_or_default();
    Some((name, format!("{} {}", d.trim(), t.trim())))
}

/// Set the system clock (local time) and write it to the RTC (`hwclock -w`),
/// the way the vendor clock menu did. Needs root (the launcher).
pub fn set_local_time(t: &DateTime) -> Result<(), String> {
    let s = format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", t.year, t.month, t.day, t.hour, t.minute, t.second);
    if sim() {
        return Ok(());
    }
    let ok = |args: &[&str]| {
        std::process::Command::new(args[0])
            .args(&args[1..])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|st| st.success())
            .unwrap_or(false)
    };
    if !ok(&["date", "-s", &s]) {
        return Err("date -s failed".into());
    }
    if rtc_info().is_none() {
        return Err("time set; no RTC (rtc0) to save it".into());
    }
    if !ok(&["hwclock", "-w"]) {
        return Err("time set; hwclock -w failed".into());
    }
    Ok(())
}

pub fn mpd_play_state() -> PlayState {
    if sim() && std::env::var_os("MPD_HOST").is_none() {
        return match std::env::var("ZPOD_SIM_PLAY").as_deref() {
            Ok("play") => PlayState::Playing,
            Ok("pause") => PlayState::Paused,
            Ok("stop") => PlayState::Stopped,
            _ => PlayState::Unknown,
        };
    }
    match zpod_mpd::Client::connect_default().and_then(|mut c| c.status()) {
        Ok(s) => match s.state.as_str() {
            "play" => PlayState::Playing,
            "pause" => PlayState::Paused,
            _ => PlayState::Stopped,
        },
        Err(_) => PlayState::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn amixer_parse() {
        let out = "Simple mixer control 'Digital',0\n  Front Left: Playback 156 [75%] [-25.50dB] [on]\n";
        assert_eq!(parse_amixer_pct(out), Some(75));
        assert_eq!(parse_amixer_pct("nothing"), None);
    }
    #[test]
    fn loopback_has_ip() {
        if !sim() {
            assert_eq!(ipv4("lo").as_deref(), Some("127.0.0.1"));
        }
    }
}
