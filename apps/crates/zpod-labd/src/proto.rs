// Copyright (C) 2026 Paul Lambert
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! Command parsing and policy checks (pure, tested).

use zpod_wlan::Adapter;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cmd {
    Ping,
    WlanMonitor { iface: String, chan: u32 },
    WlanChan { iface: String, chan: u32 },
    WlanManaged { iface: String },
    WlanStream { iface: String },
    HciStream,
    /// Arm over-the-air transmit for up to `secs` (1..=120). Required before
    /// every `wlan-tx-rid` or `hid-run`. Cleared after a successful start or
    /// when the timer expires. Never armed at boot.
    TxArm { secs: u32 },
    /// Transmit a moving TEST Remote ID beacon train on a USB adapter already
    /// in monitor mode. Requires a live arm. `secs` is 1..=60.
    WlanTxRid { iface: String, chan: u32, secs: u32 },
    /// Run a HID / DuckyScript session (writes to /dev/hidg0). Requires a live
    /// arm. The client then sends one line `run` or hangs up to cancel; the
    /// daemon does not keep scripts — the app feeds HID reports after arm.
    HidArmOk,
}

pub fn valid_iface(s: &str) -> bool {
    !s.is_empty() && s.len() <= 15 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') && !s.starts_with('-')
}

/// 2.4 GHz 1-14, 5 GHz 36-177.
pub fn valid_channel(c: u32) -> bool {
    (1..=14).contains(&c) || (36..=177).contains(&c)
}

pub fn parse(line: &str) -> Result<Cmd, String> {
    let line = line.trim();
    if line.len() > 128 {
        return Err("command too long".into());
    }
    let w: Vec<&str> = line.split_ascii_whitespace().collect();
    let iface = |s: &str| -> Result<String, String> {
        if valid_iface(s) {
            Ok(s.to_string())
        } else {
            Err(format!("bad interface name {s:?}"))
        }
    };
    let chan = |s: &str| -> Result<u32, String> {
        match s.parse::<u32>() {
            Ok(c) if valid_channel(c) => Ok(c),
            _ => Err(format!("bad channel {s:?}")),
        }
    };
    let secs = |s: &str, lo: u32, hi: u32| -> Result<u32, String> {
        match s.parse::<u32>() {
            Ok(n) if (lo..=hi).contains(&n) => Ok(n),
            _ => Err(format!("bad duration {s:?} (want {lo}..={hi})")),
        }
    };
    match w.as_slice() {
        ["ping"] => Ok(Cmd::Ping),
        ["wlan-monitor", i, c] => Ok(Cmd::WlanMonitor { iface: iface(i)?, chan: chan(c)? }),
        ["wlan-chan", i, c] => Ok(Cmd::WlanChan { iface: iface(i)?, chan: chan(c)? }),
        ["wlan-managed", i] => Ok(Cmd::WlanManaged { iface: iface(i)? }),
        ["wlan-stream", i] => Ok(Cmd::WlanStream { iface: iface(i)? }),
        ["hci-stream"] => Ok(Cmd::HciStream),
        ["tx-arm", s] => Ok(Cmd::TxArm { secs: secs(s, 1, 120)? }),
        ["wlan-tx-rid", i, c, s] => Ok(Cmd::WlanTxRid { iface: iface(i)?, chan: chan(c)?, secs: secs(s, 1, 60)? }),
        ["hid-arm-ok"] => Ok(Cmd::HidArmOk),
        [] => Err("empty command".into()),
        [c, ..] => Err(format!("unknown command {c:?}")),
    }
}

/// The adapter a Wi-Fi command may act on: it must exist, have a phy and
/// not be the onboard radio.
pub fn wlan_target<'a>(iface: &str, adapters: &'a [Adapter]) -> Result<&'a Adapter, String> {
    let a = adapters.iter().find(|a| a.iface == iface).ok_or_else(|| format!("{iface}: no such wireless interface"))?;
    if a.onboard {
        return Err(format!("{iface}: the onboard radio ({}) is never used for monitor mode", a.driver));
    }
    Ok(a)
}

/// Peer allowed: root or the app user.
pub fn peer_allowed(uid: u32, app_uid: u32) -> bool {
    uid == 0 || uid == app_uid
}

/// Frame header for stream data.
pub fn frame_header(len: usize) -> [u8; 4] {
    (len as u32).to_le_bytes()
}

/// HCI command packets for a passive LE scan (H4 type 0x01 + opcode + len + params).
pub fn hci_scan_enable(on: bool) -> [u8; 6] {
    [0x01, 0x0C, 0x20, 2, on as u8, 0]
}

pub fn hci_scan_params_passive() -> [u8; 11] {
    [0x01, 0x0B, 0x20, 7, 0x00, 0x60, 0x00, 0x30, 0x00, 0x00, 0x00]
}

/// Warning text the UI must show before every transmit (Paul 2026-11-20).
pub const TX_SAFEGUARD: &str = "isolated lab: cabled / dummy load / Faraday enclosure in place?";

#[cfg(test)]
mod tests {
    use super::*;

    fn ad(iface: &str, onboard: bool) -> Adapter {
        Adapter { iface: iface.into(), onboard, driver: if onboard { "brcmfmac".into() } else { "rt2800usb".into() }, ..Default::default() }
    }

    #[test]
    fn parses_commands() {
        assert_eq!(parse("ping\n"), Ok(Cmd::Ping));
        assert_eq!(parse("wlan-monitor wlan1 6"), Ok(Cmd::WlanMonitor { iface: "wlan1".into(), chan: 6 }));
        assert_eq!(parse("wlan-chan wlan1 149"), Ok(Cmd::WlanChan { iface: "wlan1".into(), chan: 149 }));
        assert_eq!(parse("hci-stream"), Ok(Cmd::HciStream));
        assert_eq!(parse("tx-arm 30"), Ok(Cmd::TxArm { secs: 30 }));
        assert_eq!(parse("wlan-tx-rid wlan1 6 15"), Ok(Cmd::WlanTxRid { iface: "wlan1".into(), chan: 6, secs: 15 }));
        assert_eq!(parse("hid-arm-ok"), Ok(Cmd::HidArmOk));
        assert!(parse("tx-arm 0").is_err());
        assert!(parse("tx-arm 121").is_err());
        assert!(parse("wlan-tx-rid wlan1 6 0").is_err());
        assert!(parse("wlan-monitor wlan1 0").is_err());
        assert!(parse("wlan-monitor wlan1 15").is_err());
        assert!(parse("wlan-monitor ../x 6").is_err());
        assert!(parse("wlan-monitor -x 6").is_err());
        assert!(parse("wlan-monitor wlan1;reboot 6").is_err());
        assert!(parse("wlan-inject wlan1").is_err());
        assert!(parse("").is_err());
        assert!(parse(&"x".repeat(200)).is_err());
    }

    #[test]
    fn onboard_radio_refused() {
        let v = vec![ad("wlan0", true), ad("wlan1", false)];
        assert!(wlan_target("wlan0", &v).unwrap_err().contains("never used"));
        assert_eq!(wlan_target("wlan1", &v).unwrap().iface, "wlan1");
        assert!(wlan_target("wlan9", &v).is_err());
    }

    #[test]
    fn peers_and_packets() {
        assert!(peer_allowed(0, 1000) && peer_allowed(1000, 1000) && !peer_allowed(1001, 1000));
        assert_eq!(frame_header(258), [2, 1, 0, 0]);
        assert_eq!(hci_scan_enable(true), [1, 0x0c, 0x20, 2, 1, 0]);
        assert_eq!(hci_scan_params_passive()[4], 0, "scan type must be passive");
        assert!(TX_SAFEGUARD.contains("Faraday"));
    }
}
