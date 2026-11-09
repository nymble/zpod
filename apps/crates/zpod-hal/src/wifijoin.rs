//! Join a WPA-PSK network with a password typed on the device (Buildroot
//! image; docs/USB-HOST-PLAN.md §7). Secret handling:
//!
//! * the passphrase only travels over a pipe to `zpod-wpa-psk` (stdin, never
//!   argv or the environment) and only the derived 256-bit PSK is stored,
//!   in `/data/etc/wpa_supplicant.conf` (root, 0600), like zpod-wifi.txt;
//! * nothing here logs the passphrase, the PSK or the SSID;
//! * a failed join removes the new network block and rejoins the network
//!   that was active before, so a wrong password never strands the device;
//! * success is reported only after association **and** an IPv4 address.

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::scan::run;
use crate::sys::sim;
use crate::wifi::{saved_networks, Saved};

pub const PSK_TOOL: &str = "/usr/lib/zpod/zpod-wpa-psk";

fn conf_path() -> String {
    std::env::var("ZPOD_WPA_CONF").unwrap_or_else(|_| "/data/etc/wpa_supplicant.conf".into())
}

/// WPA2 passphrase rule: 8..=63 printable ASCII, or 64 hex digits.
pub fn check_passphrase(p: &str) -> Result<(), &'static str> {
    let n = p.len();
    if n == 64 && p.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Ok(());
    }
    if !p.bytes().all(|b| (0x20..0x7f).contains(&b)) {
        return Err("Only plain ASCII characters");
    }
    if n < 8 {
        return Err("At least 8 characters");
    }
    if n > 63 {
        return Err("At most 63 characters");
    }
    Ok(())
}

pub fn hex(s: &[u8]) -> String {
    s.iter().map(|b| format!("{b:02x}")).collect()
}

/// The network block appended to wpa_supplicant.conf (SSID hex-encoded).
pub fn network_block(ssid: &str, psk_hex: &str, hidden: bool) -> String {
    let mut b = format!("\nnetwork={{\n\tssid={}\n", hex(ssid.as_bytes()));
    if hidden {
        b.push_str("\tscan_ssid=1\n");
    }
    b.push_str(&format!("\tkey_mgmt=WPA-PSK\n\tpsk={psk_hex}\n}}\n"));
    b
}

fn wipe(s: &mut String) {
    // SAFETY: zero bytes are valid UTF-8
    for b in unsafe { s.as_bytes_mut() } {
        unsafe { std::ptr::write_volatile(b, 0) };
    }
    s.clear();
}

/// passphrase -> 64 hex digits via zpod-wpa-psk (stdin only).
fn derive_psk(ssid: &str, pass: &str) -> Result<String, String> {
    if pass.len() == 64 {
        return Ok(pass.to_ascii_lowercase());
    }
    let mut c = Command::new(PSK_TOOL)
        .arg(ssid)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("{PSK_TOOL}: {e}"))?;
    c.stdin.take().unwrap().write_all(pass.as_bytes()).map_err(|_| "could not derive the key".to_string())?;
    let mut out = String::new();
    c.stdout.take().unwrap().read_to_string(&mut out).map_err(|_| "could not derive the key".to_string())?;
    let _ = c.wait();
    let mut psk = out.trim().to_string();
    wipe(&mut out);
    if psk.len() != 64 || !psk.bytes().all(|b| b.is_ascii_hexdigit()) {
        wipe(&mut psk);
        return Err("could not derive the key".into());
    }
    Ok(psk)
}

fn id_of(s: &Saved) -> Option<u32> {
    s.uuid.strip_prefix("wpa-")?.parse().ok()
}

/// Wait for wlan0 to have IPv4 (DHCP), up to `limit`.
fn wait_ip(limit: Duration) -> Option<String> {
    let t = Instant::now();
    loop {
        if let Some(ip) = crate::sys::ipv4("wlan0") {
            return Some(ip);
        }
        if t.elapsed() >= limit {
            return None;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

/// Add + join. Blocks up to ~70 s (association 40 s + DHCP 20 s). On
/// success returns "Joined <ssid>: <ip>".
pub fn join_with_password(ssid: &str, pass: &str) -> Result<String, String> {
    check_passphrase(pass).map_err(String::from)?;
    if ssid.is_empty() || ssid.len() > 32 {
        return Err("Network name must be 1-32 bytes".into());
    }
    if sim() {
        return if pass == "wrongpass" { Err("Wrong password or out of range".into()) } else { Ok(format!("Joined {ssid}: 192.168.1.251")) };
    }
    if !std::path::Path::new(PSK_TOOL).exists() {
        return Err("Typing a password needs the Buildroot image".into());
    }
    let before = saved_networks()?;
    let prev = before.iter().find(|s| s.active).cloned();
    let mut psk = derive_psk(ssid, pass)?;
    let block = network_block(ssid, &psk, false);
    wipe(&mut psk);
    let conf = conf_path();
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new().append(true).create(true).mode(0o600).open(&conf).map_err(|e| format!("{conf}: {e}"))?;
        f.write_all(block.as_bytes()).map_err(|e| format!("{conf}: {e}"))?;
        f.sync_all().ok();
    }
    let mut block = block;
    wipe(&mut block);
    let _ = run(&["wpa_cli", "-p", "/run/wpa_supplicant", "-i", "wlan0", "reconfigure"], 10);
    // the new block is the highest id with this name
    let after = saved_networks()?;
    let new = after.iter().filter(|s| s.name == ssid).filter_map(|s| id_of(s).map(|i| (i, s.uuid.clone()))).max_by_key(|x| x.0);
    let Some((new_id, new_uuid)) = new else { return Err("network was not saved".into()) };
    let joined = run(&["nmcli", "--wait", "40", "connection", "up", "uuid", &new_uuid], 45);
    if joined.is_err() {
        let _ = run(&["nmcli", "connection", "delete", "uuid", &new_uuid], 15);
        // rejoin the network that was active before; ids above the removed
        // block shift down by one
        if let Some(p) = &prev {
            let uuid = match id_of(p) {
                Some(i) if i > new_id => format!("wpa-{}", i - 1),
                _ => p.uuid.clone(),
            };
            let _ = run(&["nmcli", "--wait", "40", "connection", "up", "uuid", &uuid], 45);
        }
        return Err("Wrong password or out of range".into());
    }
    let Some(ip) = wait_ip(Duration::from_secs(20)) else {
        return Err(format!("Joined {ssid} but got no IP address (DHCP)"));
    };
    // replace older entries with the same name (an old, wrong password)
    let mut old: Vec<u32> = after.iter().filter(|s| s.name == ssid).filter_map(id_of).filter(|&i| i != new_id).collect();
    old.sort_unstable_by(|a, b| b.cmp(a));
    for i in old {
        let _ = run(&["nmcli", "connection", "delete", "uuid", &format!("wpa-{i}")], 15);
    }
    Ok(format!("Joined {ssid}: {ip}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passphrase_rules() {
        assert!(check_passphrase("12345678").is_ok());
        assert_eq!(check_passphrase("1234567"), Err("At least 8 characters"));
        assert!(check_passphrase(&"a".repeat(63)).is_ok());
        assert_eq!(check_passphrase(&"a".repeat(65)), Err("At most 63 characters"));
        assert!(check_passphrase(&"ab".repeat(32)).is_ok(), "64 hex = raw PSK");
        assert_eq!(check_passphrase("pässwörd1"), Err("Only plain ASCII characters"));
    }

    #[test]
    fn block_hex_encodes_ssid() {
        let b = network_block("My Net", &"0".repeat(64), false);
        assert!(b.contains("ssid=4d79204e6574\n"));
        assert!(b.contains("key_mgmt=WPA-PSK"));
        assert!(!b.contains("My Net"));
        assert!(network_block("x", &"0".repeat(64), true).contains("scan_ssid=1"));
    }

    #[test]
    fn wipe_clears() {
        let mut s = String::from("secret");
        wipe(&mut s);
        assert!(s.is_empty());
    }
}
