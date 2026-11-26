// Copyright (C) 2026 Paul Lambert
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! Wireless interfaces from sysfs, and `iw phy <phy> info` capability text.

use std::fs;
use std::path::Path;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Adapter {
    pub iface: String,
    pub phy: String,
    pub driver: String,
    /// "sdio", "usb" or "?"
    pub bus: String,
    pub mac: String,
    /// true: the built-in radio (never monitor)
    pub onboard: bool,
    /// interface type from sysfs: 1 = Ethernet-like (managed/AP), 803 = radiotap (monitor)
    pub arphrd: u32,
    pub up: bool,
    /// USB id "148f:5370" for USB adapters
    pub usb_id: Option<String>,
}

impl Adapter {
    pub fn in_monitor(&self) -> bool {
        self.arphrd == 803
    }
}

fn rd(p: &Path) -> Option<String> {
    fs::read_to_string(p).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}
fn link_name(p: &Path) -> Option<String> {
    fs::read_link(p).ok().and_then(|l| l.file_name().map(|f| f.to_string_lossy().to_string()))
}

/// Onboard = the Pi's SDIO Broadcom radio. Anything else (USB) may be used
/// for monitor mode.
pub fn is_onboard(driver: &str, bus: &str) -> bool {
    driver == "brcmfmac" || bus == "sdio" || bus == "mmc"
}

/// Interfaces with a `phy80211` link under `net` (normally /sys/class/net).
pub fn inventory_at(net: &Path) -> Vec<Adapter> {
    let mut v = Vec::new();
    let Ok(rdir) = fs::read_dir(net) else { return v };
    for e in rdir.flatten() {
        let d = e.path();
        let Some(phy) = link_name(&d.join("phy80211")) else { continue };
        let driver = link_name(&d.join("device/driver")).unwrap_or_default();
        let devpath = fs::canonicalize(d.join("device")).map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
        let bus = link_name(&d.join("device/subsystem")).unwrap_or_else(|| if devpath.contains("/usb") { "usb".into() } else if devpath.contains("mmc") { "sdio".into() } else { "?".into() });
        // USB ids live on the parent device of the interface
        let usb_id = if bus == "usb" {
            let up = d.join("device/..");
            match (rd(&up.join("idVendor")), rd(&up.join("idProduct"))) {
                (Some(a), Some(b)) => Some(format!("{a}:{b}")),
                _ => None,
            }
        } else {
            None
        };
        v.push(Adapter {
            iface: e.file_name().to_string_lossy().to_string(),
            onboard: is_onboard(&driver, &bus),
            phy,
            driver,
            bus,
            mac: rd(&d.join("address")).unwrap_or_default(),
            arphrd: rd(&d.join("type")).and_then(|s| s.parse().ok()).unwrap_or(0),
            up: rd(&d.join("operstate")).is_some_and(|s| s != "down"),
            usb_id,
        });
    }
    v.sort_by(|a, b| a.iface.cmp(&b.iface));
    v
}

pub fn inventory() -> Vec<Adapter> {
    inventory_at(Path::new("/sys/class/net"))
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PhyCaps {
    pub modes: Vec<String>,
    /// usable (not disabled) frequencies in MHz
    pub freqs: Vec<u32>,
}

impl PhyCaps {
    pub fn monitor(&self) -> bool {
        self.modes.iter().any(|m| m == "monitor")
    }
    pub fn has_5ghz(&self) -> bool {
        self.freqs.iter().any(|f| *f > 4900)
    }
}

/// Parse `iw phy <phy> info`.
pub fn parse_phy_info(t: &str) -> PhyCaps {
    let mut c = PhyCaps::default();
    let mut in_modes = false;
    let mut in_freqs = false;
    for line in t.lines() {
        let s = line.trim();
        if s.starts_with("Supported interface modes:") {
            in_modes = true;
            in_freqs = false;
            continue;
        }
        if s.starts_with("Frequencies:") {
            in_freqs = true;
            in_modes = false;
            continue;
        }
        if let Some(rest) = s.strip_prefix("* ") {
            if in_modes {
                c.modes.push(rest.trim().to_string());
            } else if in_freqs {
                // "* 2412.0 MHz [1] (20.0 dBm)" / "* 5260 MHz [52] (disabled)"
                if !rest.contains("disabled") {
                    if let Some(f) = rest.split_whitespace().next().and_then(|x| x.split('.').next()).and_then(|x| x.parse::<u32>().ok()) {
                        c.freqs.push(f);
                    }
                }
            }
            continue;
        }
        // any other line ends a "* " list
        if !s.is_empty() {
            in_modes = false;
            if !s.starts_with("Frequencies") {
                in_freqs = false;
            }
        }
    }
    c
}

pub fn freq_to_channel(f: u32) -> u32 {
    match f {
        2484 => 14,
        2412..=2472 => (f - 2407) / 5,
        5000..=5900 => (f - 5000) / 5,
        5955..=7115 => (f - 5950) / 5,
        _ => 0,
    }
}
