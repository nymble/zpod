//! Wi-Fi tools (M6): link signal, channel congestion, saved networks and
//! switching, hotspot (access point) mode. Root only (the launcher).
//!
//! The Zero W radio (BCM43430) cannot be a client and an access point at
//! the same time with the stock driver/NetworkManager, so hotspot mode takes
//! the ZPOD off the home network until it is turned off again.
//!
//! Hotspot: a NetworkManager keyfile `zpod-hotspot` (0600, never in the
//! image or the repo) written on first use: SSID `ZPOD-<last 4 hex of the
//! wlan0 MAC>`, WPA2-PSK (RSN/CCMP) with a passphrase generated on the
//! device from /dev/urandom, IPv4 shared 10.42.0.1/24 (DHCP for clients),
//! 2.4 GHz on the least busy of channels 1/6/11 from the last scan.
//!
//! test3: one Wi-Fi mode (Off / Client / Hotspot) switched explicitly with
//! [`set_mode`]; joining an open network, deleting a saved one, the hotspot's
//! connected devices (station dump + DHCP leases + ARP). Everything goes
//! through `nmcli`, which is NetworkManager on Pi OS and a shim on the
//! Buildroot image (wpa_supplicant / hostapd + udhcpd / iw).

use std::path::PathBuf;

use crate::scan::{run, WifiNet};
use crate::sys::sim;

pub const HOTSPOT_ID: &str = "zpod-hotspot";
pub const HOTSPOT_IP: &str = "10.42.0.1";

/// Split one `nmcli -t` line (':' separated, '\:' and '\\' escaped).
pub fn split_terse(line: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut esc = false;
    for c in line.chars() {
        match (esc, c) {
            (true, _) => {
                out.last_mut().unwrap().push(c);
                esc = false;
            }
            (false, '\\') => esc = true,
            (false, ':') => out.push(String::new()),
            (false, _) => out.last_mut().unwrap().push(c),
        }
    }
    out
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Saved {
    pub name: String,
    pub uuid: String,
    pub autoconnect: bool,
    pub active: bool,
}

/// `nmcli -t -f NAME,UUID,TYPE,AUTOCONNECT,ACTIVE connection show` -> Wi-Fi
/// client profiles (the hotspot profile is left out).
pub fn parse_saved(text: &str) -> Vec<Saved> {
    let mut v: Vec<Saved> = text
        .lines()
        .filter_map(|l| {
            let f = split_terse(l);
            if f.len() < 5 || !(f[2] == "802-11-wireless" || f[2] == "wifi") || f[0] == HOTSPOT_ID {
                return None;
            }
            Some(Saved { name: f[0].clone(), uuid: f[1].clone(), autoconnect: f[3] == "yes", active: f[4] == "yes" })
        })
        .collect();
    v.sort_by(|a, b| (!a.active, a.name.to_lowercase()).cmp(&(!b.active, b.name.to_lowercase())));
    v
}

pub fn saved_networks() -> Result<Vec<Saved>, String> {
    if sim() {
        let active = !sim_hotspot_on();
        return Ok(vec![
            Saved { name: "HomeNet".into(), uuid: "u1".into(), autoconnect: true, active },
            Saved { name: "Phone hotspot".into(), uuid: "u2".into(), autoconnect: true, active: false },
            Saved { name: "Office".into(), uuid: "u3".into(), autoconnect: true, active: false },
        ]);
    }
    run(&["nmcli", "-t", "-f", "NAME,UUID,TYPE,AUTOCONNECT,ACTIVE", "connection", "show"], 10).map(|t| parse_saved(&t))
}

/// Forget a saved network (`nmcli connection delete uuid U`).
pub fn delete_saved(uuid: &str) -> Result<(), String> {
    if sim() {
        return Ok(());
    }
    run(&["nmcli", "connection", "delete", "uuid", uuid], 15).map(|_| ())
}

/// Join an open (no password) network seen in a scan; NetworkManager (or the
/// shim) saves it. Networks with a password are added with zpod-wifi.txt.
pub fn join_open(ssid: &str) -> Result<(), String> {
    if sim() {
        set_sim_hotspot(false);
        return Ok(());
    }
    run(&["nmcli", "--wait", "40", "device", "wifi", "connect", ssid, "ifname", "wlan0"], 45).map(|_| ())
}

/// Switch to a saved network (also turns the hotspot off). Blocks up to 45 s.
pub fn connect(uuid: &str) -> Result<(), String> {
    if sim() {
        set_sim_hotspot(false);
        return Ok(());
    }
    run(&["nmcli", "--wait", "40", "connection", "up", "uuid", uuid], 45).map(|_| ())
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Link {
    pub bssid: String,
    pub ssid: String,
    pub freq_mhz: u32,
    pub signal_dbm: i32,
    pub tx_mbit: Option<f32>,
}

/// `iw dev wlan0 link`
pub fn parse_link(text: &str) -> Option<Link> {
    let mut l = Link::default();
    for line in text.lines() {
        let t = line.trim();
        if let Some(r) = t.strip_prefix("Connected to ") {
            l.bssid = r.split_whitespace().next().unwrap_or("").to_string();
        } else if let Some(r) = t.strip_prefix("SSID: ") {
            l.ssid = r.to_string();
        } else if let Some(r) = t.strip_prefix("freq: ") {
            l.freq_mhz = r.split(['.', ' ']).next().and_then(|x| x.parse().ok()).unwrap_or(0);
        } else if let Some(r) = t.strip_prefix("signal: ") {
            l.signal_dbm = r.split_whitespace().next().and_then(|x| x.parse().ok()).unwrap_or(-127);
        } else if let Some(r) = t.strip_prefix("tx bitrate: ") {
            l.tx_mbit = r.split_whitespace().next().and_then(|x| x.parse().ok());
        }
    }
    (!l.bssid.is_empty()).then_some(l)
}

/// Current association (cheap: no scan). None when not connected.
pub fn link() -> Option<Link> {
    if sim() {
        if sim_hotspot_on() {
            return None;
        }
        // a gently wandering signal for the meter screenshots
        let t = crate::sys::now_local().second as i32;
        return Some(Link { bssid: "e0:63:da:00:00:01".into(), ssid: "HomeNet".into(), freq_mhz: 2462, signal_dbm: -52 - (t % 7), tx_mbit: Some(65.0) });
    }
    run(&["iw", "dev", "wlan0", "link"], 4).ok().as_deref().and_then(parse_link)
}

/// Signal quality 0..100 from dBm (-90 -> 0, -30 -> 100), like NM's.
pub fn quality(dbm: i32) -> u8 {
    ((dbm + 90).clamp(0, 60) * 100 / 60) as u8
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Channel {
    pub ch: u32,
    /// networks on exactly this channel
    pub aps: u32,
    /// strongest of them (dBm)
    pub best_dbm: Option<f32>,
    /// overlap-weighted load: neighbours within 4 channels count partly,
    /// strong ones more than weak ones
    pub load: f32,
}

/// Per-channel view of a scan, 2.4 GHz channels 1..=13 (the Zero W radio is
/// 2.4 GHz only; 5 GHz rows in a scan are ignored).
pub fn congestion(nets: &[WifiNet]) -> Vec<Channel> {
    let two4: Vec<&WifiNet> = nets.iter().filter(|n| (1..=14).contains(&n.channel) && n.freq_mhz < 2500).collect();
    (1..=13u32)
        .map(|ch| {
            let mut c = Channel { ch, ..Default::default() };
            for n in &two4 {
                let d = (n.channel as i32 - ch as i32).unsigned_abs();
                let strength = ((n.signal_dbm + 95.0) / 60.0).clamp(0.05, 1.0);
                if d == 0 {
                    c.aps += 1;
                    c.best_dbm = Some(c.best_dbm.map_or(n.signal_dbm, |b: f32| b.max(n.signal_dbm)));
                }
                if d < 5 {
                    c.load += strength * (1.0 - d as f32 / 5.0);
                }
            }
            c
        })
        .collect()
}

/// The least loaded of the non-overlapping channels 1, 6, 11.
pub fn best_channel(ch: &[Channel]) -> u32 {
    [1u32, 6, 11]
        .into_iter()
        .min_by(|a, b| {
            let la = ch.iter().find(|c| c.ch == *a).map_or(0.0, |c| c.load);
            let lb = ch.iter().find(|c| c.ch == *b).map_or(0.0, |c| c.load);
            la.partial_cmp(&lb).unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or(6)
}

// ----------------------------------------------------------------- hotspot

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Hotspot {
    pub ssid: String,
    pub psk: String,
    pub channel: u32,
    pub active: bool,
    pub clients: u32,
}

const PSK_ALPHABET: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";

/// 16 characters from an unambiguous alphabet in groups of 4
/// ("k7mq-p3xa-9rtw-e4hn", ~79 bits), from `rand` bytes.
pub fn passphrase_from(rand: &[u8]) -> String {
    let mut s = String::new();
    let mut i = 0;
    for b in rand {
        // rejection sampling keeps it uniform
        if (*b as usize) >= 256 - 256 % PSK_ALPHABET.len() {
            continue;
        }
        if i > 0 && i % 4 == 0 {
            s.push('-');
        }
        s.push(PSK_ALPHABET[*b as usize % PSK_ALPHABET.len()] as char);
        i += 1;
        if i == 16 {
            break;
        }
    }
    s
}

pub fn new_passphrase() -> Result<String, String> {
    use std::io::Read;
    let mut b = [0u8; 64];
    std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut b)).map_err(|e| format!("urandom: {e}"))?;
    let p = passphrase_from(&b);
    (p.len() == 19).then_some(p).ok_or_else(|| "passphrase generation failed".into())
}

/// "ZPOD-1a2b" from the wlan0 MAC.
pub fn hotspot_ssid(mac: Option<&str>) -> String {
    let hex: String = mac.unwrap_or("").chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if hex.len() >= 4 {
        format!("ZPOD-{}", hex[hex.len() - 4..].to_ascii_lowercase())
    } else {
        "ZPOD-hotspot".into()
    }
}

fn conn_dir() -> PathBuf {
    std::env::var_os("ZPOD_NM_CONN_DIR").map(Into::into).unwrap_or_else(|| "/etc/NetworkManager/system-connections".into())
}

fn keyfile_path() -> PathBuf {
    conn_dir().join(format!("{HOTSPOT_ID}.nmconnection"))
}

pub fn keyfile(ssid: &str, psk: &str, channel: u32, uuid: &str) -> String {
    format!(
        "# ZPOD hotspot (written on the device by the launcher; the passphrase was generated here)\n\
[connection]\nid={HOTSPOT_ID}\nuuid={uuid}\ntype=wifi\ninterface-name=wlan0\nautoconnect=false\n\n\
[wifi]\nmode=ap\nssid={ssid}\nband=bg\nchannel={channel}\npowersave=2\n\n\
[wifi-security]\nkey-mgmt=wpa-psk\nproto=rsn\npairwise=ccmp\ngroup=ccmp\npsk={psk}\n\n\
[ipv4]\nmethod=shared\naddress1={HOTSPOT_IP}/24\n\n[ipv6]\nmethod=disabled\n"
    )
}

pub fn parse_keyfile(text: &str) -> Option<Hotspot> {
    let get = |k: &str| text.lines().find_map(|l| l.trim().strip_prefix(k).map(|v| v.to_string()));
    Some(Hotspot { ssid: get("ssid=")?, psk: get("psk=")?, channel: get("channel=").and_then(|c| c.parse().ok()).unwrap_or(6), active: false, clients: 0 })
}

fn sim_state_path() -> PathBuf {
    PathBuf::from(std::env::var_os("ZPOD_SIM").unwrap_or_else(|| "/tmp".into())).join(".sim-hotspot")
}
fn sim_hotspot_on() -> bool {
    sim() && sim_state_path().exists()
}
fn set_sim_hotspot(on: bool) {
    if on {
        let _ = std::fs::write(sim_state_path(), "1");
    } else {
        let _ = std::fs::remove_file(sim_state_path());
    }
}

/// The hotspot profile (if created) and whether it is up.
pub fn hotspot() -> Option<Hotspot> {
    if sim() {
        return Some(Hotspot { ssid: "ZPOD-0001".into(), psk: "k7mq-p3xa-9rtw-e4hn".into(), channel: 1, active: sim_hotspot_on(), clients: if sim_hotspot_on() { 2 } else { 0 } });
    }
    let mut h = parse_keyfile(&std::fs::read_to_string(keyfile_path()).ok()?)?;
    h.active = run(&["nmcli", "-t", "-f", "NAME", "connection", "show", "--active"], 5).map(|t| t.lines().any(|l| l == HOTSPOT_ID)).unwrap_or(false);
    if h.active {
        h.clients = run(&["iw", "dev", "wlan0", "station", "dump"], 4).map(|t| t.lines().filter(|l| l.starts_with("Station ")).count() as u32).unwrap_or(0);
    }
    Some(h)
}

fn write_keyfile(h: &Hotspot) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let uuid = std::fs::read_to_string("/proc/sys/kernel/random/uuid").map(|s| s.trim().to_string()).unwrap_or_else(|_| "6f1d3c2a-0000-4000-8000-0000000020d0".into());
    let p = keyfile_path();
    let tmp = p.with_extension("tmp");
    let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
    f.write_all(keyfile(&h.ssid, &h.psk, h.channel, &uuid).as_bytes()).map_err(|e| e.to_string())?;
    drop(f);
    std::fs::rename(&tmp, &p).map_err(|e| e.to_string())?;
    run(&["nmcli", "connection", "reload"], 10).map(|_| ())
}

/// Turn the hotspot on (creating the profile with a fresh passphrase on
/// first use). `channel` = preferred channel from the congestion view.
pub fn hotspot_on(channel: Option<u32>) -> Result<Hotspot, String> {
    if sim() {
        set_sim_hotspot(true);
        return hotspot().ok_or_else(|| "sim".into());
    }
    let mut h = match hotspot() {
        Some(h) => h,
        None => Hotspot { ssid: hotspot_ssid(crate::sys::mac_address("wlan0").as_deref()), psk: new_passphrase()?, channel: channel.unwrap_or(6), ..Default::default() },
    };
    if let Some(c) = channel {
        h.channel = c;
    }
    write_keyfile(&h)?;
    run(&["nmcli", "--wait", "30", "connection", "up", HOTSPOT_ID], 35)?;
    Ok(hotspot().unwrap_or(h))
}

/// Hotspot off; NetworkManager then reconnects a saved home network.
pub fn hotspot_off() -> Result<(), String> {
    if sim() {
        set_sim_hotspot(false);
        return Ok(());
    }
    run(&["nmcli", "connection", "down", HOTSPOT_ID], 15)?;
    let _ = run(&["nmcli", "device", "connect", "wlan0"], 30);
    Ok(())
}

/// New passphrase (old one stops working; clients must re-join).
pub fn hotspot_new_passphrase() -> Result<Hotspot, String> {
    if sim() {
        return hotspot().ok_or_else(|| "sim".into());
    }
    let mut h = hotspot().ok_or("hotspot not set up yet")?;
    h.psk = new_passphrase()?;
    let was = h.active;
    write_keyfile(&h)?;
    if was {
        run(&["nmcli", "--wait", "30", "connection", "up", HOTSPOT_ID], 35)?;
    }
    Ok(hotspot().unwrap_or(h))
}

// -------------------------------------------------------------------- mode

/// The one Wi-Fi mode the UI shows and switches (the Zero W radio is either
/// a client or an access point, never both).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Mode {
    /// radio off (`nmcli radio wifi off`)
    Off,
    /// joins saved networks
    #[default]
    Client,
    /// ZPOD is the access point
    Hotspot,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Mode::Off => "Off",
            Mode::Client => "Client",
            Mode::Hotspot => "Hotspot",
        }
    }
    pub const ALL: [Mode; 3] = [Mode::Off, Mode::Client, Mode::Hotspot];
}

fn sim_mode_path() -> PathBuf {
    PathBuf::from(std::env::var_os("ZPOD_SIM").unwrap_or_else(|| "/tmp".into())).join(".sim-wifi-off")
}

/// `nmcli radio wifi` -> "enabled" / "disabled".
pub fn radio_on() -> bool {
    if sim() {
        return !sim_mode_path().exists() && std::env::var("ZPOD_SIM_WIFI_MODE").as_deref() != Ok("off");
    }
    run(&["nmcli", "radio", "wifi"], 5).map(|t| !t.trim().starts_with("disabled")).unwrap_or(true)
}

/// Current mode (one nmcli call, plus one for the hotspot when the radio is on).
pub fn mode() -> Mode {
    if !radio_on() {
        return Mode::Off;
    }
    if sim() {
        return if sim_hotspot_on() || std::env::var("ZPOD_SIM_WIFI_MODE").as_deref() == Ok("hotspot") { Mode::Hotspot } else { Mode::Client };
    }
    let active = run(&["nmcli", "-t", "-f", "NAME", "connection", "show", "--active"], 5).unwrap_or_default();
    if active.lines().any(|l| l == HOTSPOT_ID) {
        Mode::Hotspot
    } else {
        Mode::Client
    }
}

/// Switch mode. Off: hotspot down, radio off (no wpa_supplicant/hostapd
/// left running). Client: radio on, hotspot down, rejoin saved networks.
/// Hotspot: radio on, then the hotspot (profile made on first use).
pub fn set_mode(m: Mode, channel: Option<u32>) -> Result<String, String> {
    if sim() {
        match m {
            Mode::Off => {
                set_sim_hotspot(false);
                let _ = std::fs::write(sim_mode_path(), "1");
            }
            Mode::Client => {
                set_sim_hotspot(false);
                let _ = std::fs::remove_file(sim_mode_path());
            }
            Mode::Hotspot => {
                let _ = std::fs::remove_file(sim_mode_path());
                set_sim_hotspot(true);
            }
        }
        return Ok(format!("Wi-Fi: {}", m.label()));
    }
    match m {
        Mode::Off => {
            let _ = run(&["nmcli", "connection", "down", HOTSPOT_ID], 15);
            run(&["nmcli", "radio", "wifi", "off"], 15)?;
            Ok("Wi-Fi off".into())
        }
        Mode::Client => {
            run(&["nmcli", "radio", "wifi", "on"], 15)?;
            let _ = run(&["nmcli", "connection", "down", HOTSPOT_ID], 15);
            let _ = run(&["nmcli", "device", "connect", "wlan0"], 30);
            Ok(match link() {
                Some(l) => format!("Connected to {}", l.ssid),
                None => "Client: looking for saved networks".into(),
            })
        }
        Mode::Hotspot => {
            run(&["nmcli", "radio", "wifi", "on"], 15)?;
            hotspot_on(channel).map(|h| format!("Hotspot {} on", h.ssid))
        }
    }
}

/// A device on the hotspot (or a neighbour on the LAN).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Peer {
    pub mac: String,
    pub ip: Option<String>,
    pub name: Option<String>,
    pub signal_dbm: Option<i32>,
    pub connected_s: Option<u64>,
}

/// `iw dev wlan0 station dump` -> (MAC, signal, connected time).
pub fn parse_stations(text: &str) -> Vec<Peer> {
    let mut v: Vec<Peer> = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if let Some(r) = t.strip_prefix("Station ") {
            v.push(Peer { mac: r.split_whitespace().next().unwrap_or("").to_lowercase(), ..Default::default() });
        } else if let Some(p) = v.last_mut() {
            if let Some(r) = t.strip_prefix("signal:") {
                p.signal_dbm = r.split_whitespace().next().and_then(|x| x.parse().ok());
            } else if let Some(r) = t.strip_prefix("connected time:") {
                p.connected_s = r.split_whitespace().next().and_then(|x| x.parse().ok());
            }
        }
    }
    v
}

/// /proc/net/arp rows for `ifname`: (ip, mac). Incomplete entries are skipped.
pub fn parse_arp(text: &str, ifname: &str) -> Vec<(String, String)> {
    text.lines()
        .skip(1)
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            (f.len() >= 6 && f[5] == ifname && f[3] != "00:00:00:00:00:00" && f[2] != "0x0").then(|| (f[0].to_string(), f[3].to_lowercase()))
        })
        .collect()
}

/// DHCP leases as text: dnsmasq (`expiry mac ip host id`, Pi OS / NM shared)
/// or BusyBox `dumpleases` (`Mac Address IP Address Host Name Expires in`).
/// -> (mac, ip, hostname).
pub fn parse_leases(text: &str) -> Vec<(String, String, Option<String>)> {
    text.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            if f.len() < 3 {
                return None;
            }
            let is_mac = |s: &str| s.len() == 17 && s.matches(':').count() == 5;
            let host = |s: Option<&&str>| s.filter(|h| **h != "*" && !h.is_empty()).map(|h| h.to_string());
            if is_mac(f[1]) && f[0].chars().all(|c| c.is_ascii_digit()) {
                Some((f[1].to_lowercase(), f[2].to_string(), host(f.get(3))))
            } else if is_mac(f[0]) && f[1].contains('.') {
                // dumpleases: host may be missing when the client sent none
                let h = if f.len() >= 4 { f.get(2) } else { None };
                Some((f[0].to_lowercase(), f[1].to_string(), host(h)))
            } else {
                None
            }
        })
        .collect()
}

fn lease_text() -> String {
    let mut t = String::new();
    for f in ["/var/lib/NetworkManager/dnsmasq-wlan0.leases", "/var/lib/misc/dnsmasq.leases"] {
        if let Ok(x) = std::fs::read_to_string(f) {
            t.push_str(&x);
        }
    }
    // Buildroot hotspot (binary lease file; BusyBox dumpleases prints it)
    let lf = "/run/zpod/udhcpd.wlan0.leases";
    if std::path::Path::new(lf).exists() {
        if let Ok(x) = run(&["dumpleases", "-f", lf], 3) {
            t.push_str(&x);
        }
    }
    t
}

/// Devices on the hotspot: associated stations, with IP and name from the
/// DHCP leases / ARP table when known.
pub fn hotspot_clients() -> Vec<Peer> {
    if sim() {
        if !sim_hotspot_on() {
            return Vec::new();
        }
        return vec![
            Peer { mac: "3a:1c:9e:00:00:21".into(), ip: Some("10.42.0.23".into()), name: Some("Pauls-iPad".into()), signal_dbm: Some(-41), connected_s: Some(312) },
            Peer { mac: "f2:6d:00:00:00:35".into(), ip: Some("10.42.0.57".into()), name: None, signal_dbm: Some(-63), connected_s: Some(48) },
        ];
    }
    let mut v = run(&["iw", "dev", "wlan0", "station", "dump"], 4).map(|t| parse_stations(&t)).unwrap_or_default();
    let leases = parse_leases(&lease_text());
    let arp = parse_arp(&std::fs::read_to_string("/proc/net/arp").unwrap_or_default(), "wlan0");
    for p in &mut v {
        if let Some((_, ip, h)) = leases.iter().find(|(m, _, _)| *m == p.mac) {
            p.ip = Some(ip.clone());
            p.name = h.clone();
        } else if let Some((ip, _)) = arp.iter().find(|(_, m)| *m == p.mac) {
            p.ip = Some(ip.clone());
        }
    }
    v
}

/// LAN neighbours seen on wlan0 (ARP), for Wi-Fi tools > Devices.
pub fn neighbours() -> Vec<(String, String)> {
    if sim() {
        return vec![("192.168.1.1".into(), "e0:63:da:00:00:01".into()), ("192.168.1.23".into(), "3a:1c:9e:00:00:21".into()), ("192.168.1.40".into(), "b0:be:76:00:00:44".into())];
    }
    parse_arp(&std::fs::read_to_string("/proc/net/arp").unwrap_or_default(), "wlan0")
}

/// Change the hotspot password to a new random one (alias kept for the UI).
pub fn hotspot_change_password() -> Result<Hotspot, String> {
    hotspot_new_passphrase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terse_split_and_saved() {
        assert_eq!(split_terse(r"a\:b:c\\d:"), vec!["a:b", r"c\d", ""]);
        let t = "usb-gadget-shared:1111:802-3-ethernet:yes:yes\nHome\\:Net:2222:802-11-wireless:yes:yes\nzpod-hotspot:3333:802-11-wireless:no:no\nCafe:4444:802-11-wireless:no:no\n";
        let v = parse_saved(t);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0], Saved { name: "Home:Net".into(), uuid: "2222".into(), autoconnect: true, active: true });
        assert_eq!(v[1].name, "Cafe");
    }

    #[test]
    fn link_parse() {
        let t = "Connected to e0:63:da:00:00:01 (on wlan0)\n\tSSID: HomeNet\n\tfreq: 2462.0\n\tRX: 1 bytes\n\tsignal: -52 dBm\n\ttx bitrate: 65.0 MBit/s MCS 7\n";
        let l = parse_link(t).unwrap();
        assert_eq!((l.ssid.as_str(), l.freq_mhz, l.signal_dbm, l.tx_mbit), ("HomeNet", 2462, -52, Some(65.0)));
        assert_eq!(parse_link("Not connected.\n"), None);
        assert_eq!(quality(-30), 100);
        assert_eq!(quality(-60), 50);
        assert_eq!(quality(-95), 0);
    }

    #[test]
    fn congestion_and_best_channel() {
        let n = |ch: u32, s: f32| WifiNet { channel: ch, freq_mhz: 2407 + 5 * ch, signal_dbm: s, ..Default::default() };
        let mut v = vec![n(6, -40.0), n(6, -50.0), n(7, -60.0), n(11, -80.0)];
        v.push(WifiNet { channel: 36, freq_mhz: 5180, signal_dbm: -30.0, ..Default::default() });
        let c = congestion(&v);
        assert_eq!(c.len(), 13);
        assert_eq!(c[5].aps, 2);
        assert_eq!(c[5].best_dbm, Some(-40.0));
        assert_eq!(c[0].aps, 0);
        assert!(c[5].load > c[10].load && c[10].load > c[0].load);
        assert_eq!(best_channel(&c), 1);
        assert_eq!(best_channel(&congestion(&[])), 1);
    }

    #[test]
    fn stations_leases_arp() {
        let st = "Station 3a:1c:9e:00:00:21 (on wlan0)\n\tinactive time:\t30 ms\n\tsignal:  \t-41 [-41] dBm\n\tconnected time:\t312 seconds\nStation F2:6D:00:00:00:35 (on wlan0)\n\tsignal:  \t-63 dBm\n";
        let v = parse_stations(st);
        assert_eq!(v.len(), 2);
        assert_eq!((v[0].mac.as_str(), v[0].signal_dbm, v[0].connected_s), ("3a:1c:9e:00:00:21", Some(-41), Some(312)));
        assert_eq!(v[1].mac, "f2:6d:00:00:00:35");
        let dnsmasq = "1767312000 3a:1c:9e:00:00:21 10.42.0.23 Pauls-iPad 01:3a:1c\n1767312000 f2:6d:00:00:00:35 10.42.0.57 * *\n";
        let l = parse_leases(dnsmasq);
        assert_eq!(l[0], ("3a:1c:9e:00:00:21".into(), "10.42.0.23".into(), Some("Pauls-iPad".into())));
        assert_eq!(l[1].2, None);
        let dl = "Mac Address       IP Address      Host Name           Expires in\n3a:1c:9e:00:00:21 10.42.0.23      Pauls-iPad          0:59:12\nf2:6d:00:00:00:35 10.42.0.57                          0:58:40\n";
        let l = parse_leases(dl);
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].2.as_deref(), Some("Pauls-iPad"));
        assert_eq!(l[1].2, None);
        let arp = "IP address       HW type     Flags       HW address            Mask     Device\n10.42.0.23       0x1         0x2         3a:1c:9e:00:00:21     *        wlan0\n10.12.194.2      0x1         0x2         aa:bb:cc:00:00:01     *        usb0\n10.42.0.99       0x1         0x0         00:00:00:00:00:00     *        wlan0\n";
        assert_eq!(parse_arp(arp, "wlan0"), vec![("10.42.0.23".to_string(), "3a:1c:9e:00:00:21".to_string())]);
        assert_eq!(parse_arp(arp, "usb0").len(), 1);
        assert_eq!(Mode::ALL.map(|m| m.label()), ["Off", "Client", "Hotspot"]);
    }

    #[test]
    fn hotspot_bits() {
        let p = passphrase_from(&(0u8..=255).collect::<Vec<_>>());
        assert_eq!(p.len(), 19);
        assert_eq!(p.matches('-').count(), 3);
        assert!(p.chars().all(|c| c == '-' || PSK_ALPHABET.contains(&(c as u8))));
        assert!(passphrase_from(&[250u8; 64]).is_empty(), "rejected bytes are skipped");
        assert_eq!(hotspot_ssid(Some("b8:27:eb:12:1A:2b")), "ZPOD-1a2b");
        assert_eq!(hotspot_ssid(None), "ZPOD-hotspot");
        let k = keyfile("ZPOD-1a2b", "abcd-efgh-jkmn-pqrs", 11, "u");
        assert!(k.contains("mode=ap\n") && k.contains("method=shared\n") && k.contains("address1=10.42.0.1/24\n") && k.contains("autoconnect=false\n"));
        let h = parse_keyfile(&k).unwrap();
        assert_eq!((h.ssid.as_str(), h.psk.as_str(), h.channel), ("ZPOD-1a2b", "abcd-efgh-jkmn-pqrs", 11));
    }
}
