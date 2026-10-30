//! Wi-Fi and Bluetooth scanners for Tools (M5). Parsers are pure and tested;
//! the scan functions shell out to `iw`, `nmcli`, `btmgmt` and `bluetoothctl`.
//!
//! Privilege: triggering a Wi-Fi scan (`iw dev wlan0 scan`) and Bluetooth
//! discovery through the kernel management socket (`btmgmt find`) need
//! CAP_NET_ADMIN. The launcher already runs as root (it owns the panel and
//! the power actions), so the scans run inside the launcher; no setuid helper
//! or extra capability is installed. Fallbacks without root: Wi-Fi asks
//! NetworkManager to rescan (`nmcli device wifi rescan`, netdev group) and
//! reads `iw dev wlan0 scan dump`; Bluetooth uses `bluetoothctl` over D-Bus.
//!
//! Both scanners are passive listeners for the user's own environment: they
//! list what devices broadcast, never connect, pair or send anything else.

use std::process::{Command, Stdio};
use std::time::Duration;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WifiNet {
    pub ssid: String,
    pub bssid: String,
    pub freq_mhz: u32,
    pub channel: u32,
    pub signal_dbm: f32,
    /// "Open", "WEP", "WPA", "WPA2", "WPA3", "WPA2/WPA3", "WPA2-EAP"...
    pub security: String,
    pub associated: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum WifiSort {
    #[default]
    Signal,
    Name,
    Channel,
}

impl WifiSort {
    pub fn next(self) -> Self {
        match self {
            WifiSort::Signal => WifiSort::Name,
            WifiSort::Name => WifiSort::Channel,
            WifiSort::Channel => WifiSort::Signal,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            WifiSort::Signal => "signal",
            WifiSort::Name => "name",
            WifiSort::Channel => "channel",
        }
    }
}

/// 2.4 / 5 / 6 GHz frequency to channel number.
pub fn freq_to_channel(f: u32) -> u32 {
    match f {
        2484 => 14,
        2412..=2472 => (f - 2407) / 5,
        5955..=7115 => (f - 5950) / 5,
        5000..=5925 => (f - 5000) / 5,
        _ => 0,
    }
}

fn security_of(privacy: bool, rsn_auth: Option<&str>, wpa: bool) -> String {
    match rsn_auth {
        Some(a) => {
            let sae = a.contains("SAE");
            let psk = a.split_whitespace().any(|w| w == "PSK" || w == "PSK/SHA-256");
            let eap = a.contains("802.1X");
            match (psk, sae, eap) {
                (true, true, _) => "WPA2/WPA3".into(),
                (false, true, _) => "WPA3".into(),
                (_, _, true) => "WPA2-EAP".into(),
                (true, _, _) if wpa => "WPA/WPA2".into(),
                _ => "WPA2".into(),
            }
        }
        None if wpa => "WPA".into(),
        None if privacy => "WEP".into(),
        None => "Open".into(),
    }
}

/// Parse `iw dev wlan0 scan` / `scan dump` output.
pub fn parse_iw_scan(text: &str) -> Vec<WifiNet> {
    struct Cur {
        n: WifiNet,
        privacy: bool,
        wpa: bool,
        rsn_auth: Option<String>,
    }
    fn finish(c: Cur, out: &mut Vec<WifiNet>) {
        let mut n = c.n;
        n.security = security_of(c.privacy, c.rsn_auth.as_deref(), c.wpa);
        if n.channel == 0 {
            n.channel = freq_to_channel(n.freq_mhz);
        }
        out.push(n);
    }
    let mut out = Vec::new();
    let mut cur: Option<Cur> = None;
    let mut section = String::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("BSS ") {
            if let Some(c) = cur.take() {
                finish(c, &mut out);
            }
            let bssid: String = rest.chars().take_while(|c| c.is_ascii_hexdigit() || *c == ':').collect();
            cur = Some(Cur {
                n: WifiNet { bssid: bssid.to_lowercase(), associated: rest.contains("associated"), ..Default::default() },
                privacy: false,
                wpa: false,
                rsn_auth: None,
            });
            section.clear();
            continue;
        }
        let Some(c) = cur.as_mut() else { continue };
        let top = line.starts_with('\t') && !line.starts_with("\t\t") && !line.starts_with("\t *");
        let l = line.trim();
        if top {
            section = l.split(':').next().unwrap_or("").to_string();
            if let Some(v) = l.strip_prefix("freq:") {
                c.n.freq_mhz = v.trim().parse::<f32>().map(|f| f as u32).unwrap_or(0);
            } else if let Some(v) = l.strip_prefix("signal:") {
                c.n.signal_dbm = v.trim().trim_end_matches("dBm").trim().parse().unwrap_or(-100.0);
            } else if let Some(v) = l.strip_prefix("SSID:") {
                c.n.ssid = v.trim().to_string();
            } else if let Some(v) = l.strip_prefix("DS Parameter set: channel") {
                c.n.channel = v.trim().parse().unwrap_or(0);
            } else if l.starts_with("capability:") {
                c.privacy = l.contains("Privacy");
            } else if l.starts_with("WPA:") {
                c.wpa = true;
            }
        }
        // "RSN:\t * Version: 1" puts the first item on the header line
        if let Some(i) = l.find("Authentication suites:") {
            let v = l[i + "Authentication suites:".len()..].trim().to_string();
            if section == "RSN" {
                c.rsn_auth = Some(v);
            } else if section == "WPA" {
                c.wpa = true;
            }
        }
    }
    if let Some(c) = cur.take() {
        finish(c, &mut out);
    }
    out
}

pub fn sort_wifi(v: &mut [WifiNet], by: WifiSort) {
    match by {
        WifiSort::Signal => v.sort_by(|a, b| b.signal_dbm.partial_cmp(&a.signal_dbm).unwrap_or(std::cmp::Ordering::Equal)),
        WifiSort::Name => v.sort_by(|a, b| {
            (a.ssid.is_empty(), a.ssid.to_lowercase(), -(a.signal_dbm as i32)).cmp(&(b.ssid.is_empty(), b.ssid.to_lowercase(), -(b.signal_dbm as i32)))
        }),
        WifiSort::Channel => v.sort_by(|a, b| (a.channel, -(a.signal_dbm as i32)).cmp(&(b.channel, -(b.signal_dbm as i32)))),
    }
}

pub(crate) fn run(cmd: &[&str], timeout_s: u32) -> Result<String, String> {
    let mut c = Command::new("timeout");
    c.arg(timeout_s.to_string()).args(cmd).stdin(Stdio::null());
    let o = c.output().map_err(|e| format!("{}: {e}", cmd[0]))?;
    let out = String::from_utf8_lossy(&o.stdout).into_owned();
    if o.status.success() {
        Ok(out)
    } else {
        let err = String::from_utf8_lossy(&o.stderr).trim().to_string();
        Err(if err.is_empty() { format!("{} failed ({})", cmd[0], o.status) } else { err })
    }
}

fn sim_wifi() -> Vec<WifiNet> {
    let mk = |ssid: &str, bssid: &str, f: u32, s: f32, sec: &str, assoc: bool| WifiNet {
        ssid: ssid.into(),
        bssid: bssid.into(),
        freq_mhz: f,
        channel: freq_to_channel(f),
        signal_dbm: s,
        security: sec.into(),
        associated: assoc,
    };
    vec![
        mk("HomeNet", "e0:63:da:00:00:01", 2462, -48.0, "WPA2", true),
        mk("HomeNet-guest", "e0:63:da:00:00:02", 2462, -51.0, "WPA2/WPA3", false),
        mk("CoffeeShop", "74:83:c2:00:00:03", 2437, -67.0, "Open", false),
        mk("", "02:11:22:00:00:04", 2412, -80.0, "WPA2", false),
        mk("Printer-Direct", "f8:d0:27:00:00:05", 2412, -73.0, "WPA2", false),
        mk("Neighbour 5G", "a4:2b:b0:00:00:06", 5180, -84.0, "WPA3", false),
        mk("FlatB-2G", "3c:84:6a:00:00:07", 2437, -71.0, "WPA2", false),
        mk("xfinitywifi", "9c:3d:cf:00:00:08", 2462, -78.0, "Open", false),
        mk("Garage", "b0:be:76:00:00:09", 2427, -82.0, "WPA2", false),
    ]
}

/// Scan for Wi-Fi networks on wlan0. Returns (networks, method note).
pub fn wifi_scan() -> Result<(Vec<WifiNet>, &'static str), String> {
    if crate::sys::sim() {
        return Ok((sim_wifi(), "sim"));
    }
    // 1. a fresh active scan (root); EBUSY (-16) while NetworkManager scans
    let mut last = String::new();
    for _ in 0..3 {
        match run(&["iw", "dev", "wlan0", "scan"], 15) {
            Ok(t) => return Ok((parse_iw_scan(&t), "iw scan")),
            Err(e) if e.contains("-16") || e.contains("busy") => {
                last = e;
                std::thread::sleep(Duration::from_millis(1500));
            }
            Err(e) => {
                last = e;
                break;
            }
        }
    }
    // 2. ask NetworkManager to rescan, then read the kernel's cached results
    let _ = run(&["nmcli", "device", "wifi", "rescan", "ifname", "wlan0"], 10);
    std::thread::sleep(Duration::from_secs(3));
    match run(&["iw", "dev", "wlan0", "scan", "dump"], 5) {
        Ok(t) if !t.trim().is_empty() => Ok((parse_iw_scan(&t), "NM rescan + cached")),
        Ok(_) => Err(if last.is_empty() { "no networks found".into() } else { last }),
        Err(e) => Err(format!("{last} / {e}")),
    }
}

// --------------------------------------------------------------- Bluetooth

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BtDev {
    pub addr: String,
    pub name: String,
    pub rssi: Option<i32>,
    /// "BR/EDR" (classic), "LE public", "LE random", or "" when unknown.
    pub kind: String,
}

impl BtDev {
    pub fn is_le(&self) -> bool {
        self.kind.starts_with("LE")
    }
}

fn merge(out: &mut Vec<BtDev>, d: BtDev) {
    match out.iter_mut().find(|x| x.addr == d.addr) {
        Some(x) => {
            if !d.name.is_empty() {
                x.name = d.name;
            }
            if d.rssi.is_some() {
                x.rssi = d.rssi;
            }
            if !d.kind.is_empty() {
                x.kind = d.kind;
            }
        }
        None => out.push(d),
    }
}

/// Parse `btmgmt find` output:
/// `hci0 dev_found: 4C:87:5D:11:22:33 type LE Random rssi -79 flags 0x0000`
/// followed by optional `name Foo` lines.
pub fn parse_btmgmt_find(text: &str) -> Vec<BtDev> {
    let mut out: Vec<BtDev> = Vec::new();
    let mut last: Option<String> = None;
    for line in text.lines() {
        let l = line.trim();
        if let Some(i) = l.find("dev_found: ") {
            let rest = &l[i + 11..];
            let mut it = rest.split_whitespace();
            let addr = it.next().unwrap_or("").to_uppercase();
            let mut kind = String::new();
            let mut rssi = None;
            if let Some(t) = rest.find(" type ") {
                let after = &rest[t + 6..];
                let end = after.find(" rssi").unwrap_or(after.len());
                kind = match after[..end].trim() {
                    "BR/EDR" => "BR/EDR".into(),
                    "LE Public" => "LE public".into(),
                    "LE Random" => "LE random".into(),
                    k => k.to_string(),
                };
            }
            if let Some(r) = rest.find("rssi ") {
                rssi = rest[r + 5..].split_whitespace().next().and_then(|v| v.parse().ok());
            }
            merge(&mut out, BtDev { addr: addr.clone(), name: String::new(), rssi, kind });
            last = Some(addr);
        } else if let Some(n) = l.strip_prefix("name ") {
            if let Some(a) = &last {
                merge(&mut out, BtDev { addr: a.clone(), name: n.trim().to_string(), ..Default::default() });
            }
        }
    }
    out
}

/// Parse `bluetoothctl` scan output (`[NEW] Device AA:.. Name`,
/// `[CHG] Device AA:.. RSSI: 0xffffffb5 (-75)` or `RSSI: -75`).
pub fn parse_bluetoothctl(text: &str) -> Vec<BtDev> {
    let mut out = Vec::new();
    for raw in text.lines() {
        // strip ANSI colour codes bluetoothctl prints even when piped
        let mut l = String::with_capacity(raw.len());
        let mut esc = false;
        for ch in raw.chars() {
            match (esc, ch) {
                (false, '\u{1b}') => esc = true,
                (true, c) if c.is_ascii_alphabetic() => esc = false,
                (true, _) => {}
                (false, c) => l.push(c),
            }
        }
        let Some(i) = l.find("Device ") else { continue };
        let rest = &l[i + 7..];
        let addr: String = rest.chars().take(17).collect::<String>().to_uppercase();
        if addr.len() != 17 || addr.matches(':').count() != 5 {
            continue;
        }
        let tail = rest[17..].trim();
        if l.contains("[NEW]") {
            let name = if tail.replace('-', ":").eq_ignore_ascii_case(&addr) { String::new() } else { tail.to_string() };
            merge(&mut out, BtDev { addr, name, ..Default::default() });
        } else if let Some(r) = tail.strip_prefix("RSSI:") {
            let v = r.trim();
            let n = match v.find('(') {
                Some(p) => v[p + 1..].trim_end_matches(')').trim().parse().ok(),
                None => v.parse().ok(),
            };
            merge(&mut out, BtDev { addr, rssi: n, ..Default::default() });
        } else if let Some(n) = tail.strip_prefix("Name:").or_else(|| tail.strip_prefix("Alias:")) {
            merge(&mut out, BtDev { addr, name: n.trim().to_string(), ..Default::default() });
        }
    }
    out
}

pub fn sort_bt(v: &mut [BtDev], by_name: bool) {
    if by_name {
        v.sort_by(|a, b| (a.name.is_empty(), a.name.to_lowercase(), a.addr.clone()).cmp(&(b.name.is_empty(), b.name.to_lowercase(), b.addr.clone())));
    } else {
        v.sort_by_key(|d| -d.rssi.unwrap_or(-127));
    }
}

/// Clear the soft block on the Bluetooth radio (Raspberry Pi OS ships it
/// blocked until something unblocks it). Needs root. Returns true if any
/// Bluetooth rfkill switch exists.
pub fn bt_unblock() -> bool {
    let mut found = false;
    if let Ok(rd) = std::fs::read_dir("/sys/class/rfkill") {
        for e in rd.flatten() {
            let p = e.path();
            if std::fs::read_to_string(p.join("type")).map(|t| t.trim() == "bluetooth").unwrap_or(false) {
                found = true;
                let _ = std::fs::write(p.join("soft"), "0");
            }
        }
    }
    found
}

fn sim_bt() -> Vec<BtDev> {
    let mk = |a: &str, n: &str, r: i32, k: &str| BtDev { addr: a.into(), name: n.into(), rssi: Some(r), kind: k.into() };
    vec![
        mk("5C:F3:70:11:22:01", "Paul's iPhone", -58, "LE random"),
        mk("00:1A:7D:DA:71:13", "MX Keys", -66, "LE public"),
        mk("F4:4E:FD:00:00:03", "BT Speaker", -71, "BR/EDR"),
        mk("7B:22:91:00:00:04", "", -88, "LE random"),
        mk("C8:FF:28:00:00:05", "Watch", -77, "LE random"),
    ]
}

/// Discover classic + LE devices for about `secs` seconds.
/// Returns (devices, method note).
pub fn bt_scan(secs: u32) -> Result<(Vec<BtDev>, &'static str), String> {
    if crate::sys::sim() {
        return Ok((sim_bt(), "sim"));
    }
    if !bt_unblock() && !std::path::Path::new("/sys/class/bluetooth/hci0").exists() {
        return Err("no Bluetooth controller (hci0)".into());
    }
    let was_on = run(&["btmgmt", "info"], 5).map(|t| t.lines().any(|l| l.contains("current settings") && l.contains("powered"))).unwrap_or(false);
    let _ = run(&["btmgmt", "power", "on"], 5);
    // btmgmt find: BR/EDR inquiry and LE scan interleaved (about 10 s), then exits
    let mut res = match run(&["btmgmt", "find"], secs + 4) {
        Ok(t) => {
            let v = parse_btmgmt_find(&t);
            Ok((v, "btmgmt find"))
        }
        // btmgmt missing, refused or hung: try bluetoothctl over D-Bus
        Err(_) => {
            let t = run(&["bluetoothctl", "--timeout", &secs.to_string(), "scan", "on"], secs + 4).unwrap_or_default();
            let v = parse_bluetoothctl(&t);
            if v.is_empty() && t.contains("NotReady") {
                Err("Bluetooth not ready (blocked or powered off)".into())
            } else {
                Ok((v, "bluetoothctl"))
            }
        }
    };
    if !was_on {
        let _ = run(&["btmgmt", "power", "off"], 5);
    }
    if let Ok((v, _)) = res.as_mut() {
        v.retain(|d| !d.addr.is_empty());
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    const IW: &str = "BSS e0:63:da:e7:2a:8f(on wlan0) -- associated\n\
\tlast seen: 69622.760s [boottime]\n\
\tfreq: 2462.0\n\
\tcapability: ESS Privacy ShortPreamble RadioMeasure (0x1031)\n\
\tsignal: -76.00 dBm\n\
\tSSID: Home Net\n\
\tDS Parameter set: channel 11\n\
\tRSN:\t * Version: 1\n\
\t\t * Group cipher: CCMP\n\
\t\t * Authentication suites: PSK\n\
\tHT operation:\n\
\t\t * primary channel: 11\n\
BSS 74:83:c2:03:1d:6a(on wlan0)\n\
\tfreq: 5180\n\
\tcapability: ESS Privacy (0x0011)\n\
\tsignal: -61.00 dBm\n\
\tSSID: \n\
\tRSN:\t * Version: 1\n\
\t\t * Authentication suites: PSK SAE\n\
BSS 02:00:00:00:00:01(on wlan0)\n\
\tfreq: 2412\n\
\tcapability: ESS (0x0001)\n\
\tsignal: -88.50 dBm\n\
\tSSID: Cafe\n\
BSS 02:00:00:00:00:02(on wlan0)\n\
\tfreq: 2437\n\
\tcapability: ESS Privacy (0x0011)\n\
\tsignal: -70.00 dBm\n\
\tSSID: Old\n\
BSS 02:00:00:00:00:03(on wlan0)\n\
\tfreq: 2437\n\
\tcapability: ESS Privacy (0x0011)\n\
\tsignal: -72.00 dBm\n\
\tSSID: Corp\n\
\tRSN:\t * Version: 1\n\
\t\t * Authentication suites: IEEE 802.1X\n";

    #[test]
    fn parses_iw_scan() {
        let v = parse_iw_scan(IW);
        assert_eq!(v.len(), 5);
        assert_eq!(v[0].bssid, "e0:63:da:e7:2a:8f");
        assert_eq!((v[0].ssid.as_str(), v[0].channel, v[0].freq_mhz, v[0].security.as_str(), v[0].associated), ("Home Net", 11, 2462, "WPA2", true));
        assert_eq!(v[0].signal_dbm, -76.0);
        assert_eq!((v[1].ssid.as_str(), v[1].channel, v[1].security.as_str()), ("", 36, "WPA2/WPA3"));
        assert_eq!((v[2].security.as_str(), v[2].channel), ("Open", 1));
        assert_eq!(v[3].security, "WEP");
        assert_eq!(v[4].security, "WPA2-EAP");
        let mut s = v.clone();
        sort_wifi(&mut s, WifiSort::Signal);
        assert_eq!(s[0].bssid, "74:83:c2:03:1d:6a");
        sort_wifi(&mut s, WifiSort::Name);
        assert_eq!(s.iter().map(|n| n.ssid.as_str()).collect::<Vec<_>>(), ["Cafe", "Corp", "Home Net", "Old", ""]);
        sort_wifi(&mut s, WifiSort::Channel);
        assert_eq!(s[0].channel, 1);
        assert_eq!(s[4].channel, 36);
        assert_eq!(freq_to_channel(2484), 14);
        assert_eq!(freq_to_channel(5955), 1);
        assert_eq!(WifiSort::Channel.next(), WifiSort::Signal);
    }

    #[test]
    fn parses_btmgmt() {
        let t = "Discovery started\nhci0 type 7 discovering on\n\
hci0 dev_found: 4c:87:5d:11:22:33 type LE Random rssi -79 flags 0x0000 \nAD flags 0x1a \neir_len 23\nname Foo Phone\n\
hci0 dev_found: F4:4E:FD:00:00:03 type BR/EDR rssi -61 flags 0x0000 \nname Speaker\n\
hci0 dev_found: 4C:87:5D:11:22:33 type LE Random rssi -70 flags 0x0004 \n\
hci0 dev_found: 00:1A:7D:DA:71:13 type LE Public rssi -90 flags 0x0000 \n\
hci0 type 7 discovering off\n";
        let v = parse_btmgmt_find(t);
        assert_eq!(v.len(), 3);
        assert_eq!(v[0], BtDev { addr: "4C:87:5D:11:22:33".into(), name: "Foo Phone".into(), rssi: Some(-70), kind: "LE random".into() });
        assert_eq!((v[1].kind.as_str(), v[1].name.as_str(), v[1].rssi), ("BR/EDR", "Speaker", Some(-61)));
        assert!(v[2].is_le() && v[2].name.is_empty());
        let mut s = v.clone();
        sort_bt(&mut s, false);
        assert_eq!(s[0].name, "Speaker");
        sort_bt(&mut s, true);
        assert_eq!(s[2].addr, "00:1A:7D:DA:71:13"); // unnamed last
    }

    #[test]
    fn parses_bluetoothctl() {
        let t = "Discovery started\n[CHG] Controller B8:27:EB:CB:E7:CD Discovering: yes\n\
\u{1b}[0;92m[NEW]\u{1b}[0m Device 4C:87:5D:11:22:33 Foo Phone\n\
[NEW] Device 7B:22:91:00:00:04 7B-22-91-00-00-04\n\
[CHG] Device 4C:87:5D:11:22:33 RSSI: 0xffffffb5 (-75)\n\
[CHG] Device 7B:22:91:00:00:04 RSSI: -90\n\
[CHG] Device 7B:22:91:00:00:04 Name: Tag\n";
        let v = parse_bluetoothctl(t);
        assert_eq!(v.len(), 2);
        assert_eq!((v[0].name.as_str(), v[0].rssi), ("Foo Phone", Some(-75)));
        assert_eq!((v[1].name.as_str(), v[1].rssi), ("Tag", Some(-90)));
    }
}
