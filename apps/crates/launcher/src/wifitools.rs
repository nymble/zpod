//! Tools > Wi-Fi state (test3): the status card (mode, link, address,
//! hotspot, connected devices, refreshed on a thread), mode switching,
//! network details with the live meter, join, forget, hotspot password.
//! Slow NetworkManager calls run on a thread; the page shows "working..."
//! until they finish.

use std::collections::VecDeque;
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

use zpod_hal::scan::WifiNet;
use zpod_hal::wifi::{self, Hotspot, Link, Mode, Peer, Saved};

use crate::tools::Scanner;

pub const METER_POINTS: usize = 90;

pub struct Job {
    rx: Receiver<Result<String, String>>,
    pub what: String,
}

impl Job {
    pub fn start(what: impl Into<String>, f: impl FnOnce() -> Result<String, String> + Send + 'static) -> Job {
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(f());
        });
        Job { rx, what: what.into() }
    }

    pub fn poll(&self) -> Option<Result<String, String>> {
        match self.rx.try_recv() {
            Ok(r) => Some(r),
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err("worker failed".into())),
        }
    }
}

/// What the meter follows: the association (None) or one scan row.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Target {
    /// empty = follow the strongest BSS with this SSID (saved networks)
    pub bssid: String,
    pub ssid: String,
    pub channel: u32,
}

/// What the details page shows and what OK (join) does.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NetDetail {
    pub ssid: String,
    pub security: String,
    pub saved_uuid: Option<String>,
    pub connected: bool,
}

/// One status snapshot for the Wi-Fi card (gathered off the UI thread).
#[derive(Clone, Debug, Default)]
pub struct Status {
    pub mode: Mode,
    pub link: Option<Link>,
    pub ip: Option<String>,
    pub mac: Option<String>,
    pub hotspot: Option<Hotspot>,
    pub clients: Vec<Peer>,
}

pub fn gather_status() -> Status {
    let mode = wifi::mode();
    let mut s = Status { mode, ip: zpod_hal::sys::ipv4("wlan0"), mac: zpod_hal::sys::mac_address("wlan0"), ..Default::default() };
    match mode {
        Mode::Off => {}
        Mode::Client => s.link = wifi::link(),
        Mode::Hotspot => {
            s.hotspot = wifi::hotspot();
            s.clients = wifi::hotspot_clients();
        }
    }
    s
}

#[derive(Default)]
pub struct WifiTools {
    pub target: Option<Target>,
    pub history: VecDeque<i32>,
    pub link: Option<Link>,
    pub last: Option<i32>,
    pub meter_at: Option<Instant>,
    pub meter_scan: Scanner<WifiNet>,
    pub saved: Vec<Saved>,
    pub saved_err: Option<String>,
    pub hotspot: Option<Hotspot>,
    pub job: Option<Job>,
    /// SSID we were on before the hotspot (for the warning text).
    pub home_ssid: Option<String>,
    /// status card (None until the first gather finishes)
    pub status: Option<Status>,
    status_rx: Option<Receiver<Status>>,
    pub status_at: Option<Instant>,
    /// details page: what OK joins
    pub detail: NetDetail,
    /// hotspot password visible (Password page, OK toggles)
    pub reveal: bool,
    /// saved list: Tri pressed once on this row (Tri again within 4 s = forget)
    pub forget_armed: Option<(usize, Instant)>,
    /// Wi-Fi tools > Devices: LAN neighbours (ip, mac)
    pub neighbours: Vec<(String, String)>,
    /// join() wants a typed password for `detail` (opens Page::WifiPassword)
    pub want_password: bool,
}

fn meter_job() -> crate::tools::ScanResult<WifiNet> {
    zpod_hal::scan::wifi_scan()
}

impl WifiTools {
    pub fn open_meter(&mut self, target: Option<Target>) {
        self.target = target;
        self.history.clear();
        self.last = None;
        self.link = None;
        self.meter_at = None;
    }

    fn push(&mut self, dbm: Option<i32>) {
        self.last = dbm;
        if let Some(d) = dbm {
            if self.history.len() >= METER_POINTS {
                self.history.pop_front();
            }
            self.history.push_back(d);
        }
    }

    /// Meter tick: the connected network once a second (iw link, no scan),
    /// a scanned network whenever a new scan finishes (~ every 4-6 s).
    /// Returns true when something new is to be drawn.
    pub fn meter_tick(&mut self) -> bool {
        match self.target.clone() {
            None => {
                if self.meter_at.is_some_and(|t| t.elapsed() < Duration::from_millis(950)) {
                    return false;
                }
                self.meter_at = Some(Instant::now());
                self.link = wifi::link();
                let d = self.link.as_ref().map(|l| l.signal_dbm);
                self.push(d);
                true
            }
            Some(t) => {
                if self.meter_scan.poll() {
                    let d = if t.bssid.is_empty() {
                        self.meter_scan.items.iter().filter(|n| n.ssid == t.ssid).map(|n| n.signal_dbm as i32).max()
                    } else {
                        self.meter_scan.items.iter().find(|n| n.bssid == t.bssid).map(|n| n.signal_dbm as i32)
                    };
                    self.push(d);
                    self.meter_at = Some(Instant::now());
                    return true;
                }
                if self.meter_scan.busy.is_none() {
                    self.meter_scan.start(meter_job);
                    return true;
                }
                false
            }
        }
    }

    /// Start a status gather unless one is running or the last is fresh.
    pub fn refresh_status(&mut self, force: bool) {
        if self.status_rx.is_some() || (!force && self.status_at.is_some_and(|t| t.elapsed() < Duration::from_secs(5))) {
            return;
        }
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(gather_status());
        });
        self.status_rx = Some(rx);
        self.status_at = Some(Instant::now());
    }

    /// Collect a finished gather; true when the card changed.
    pub fn poll_status(&mut self) -> bool {
        let Some(rx) = &self.status_rx else { return false };
        match rx.try_recv() {
            Ok(s) => {
                self.status_rx = None;
                self.hotspot = s.hotspot.clone().or(self.hotspot.take().filter(|_| s.mode == Mode::Hotspot));
                self.status = Some(s);
                true
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => false,
            Err(_) => {
                self.status_rx = None;
                false
            }
        }
    }

    pub fn mode(&self) -> Mode {
        self.status.as_ref().map(|s| s.mode).unwrap_or_default()
    }

    /// Wi-Fi > Mode: switch (hotspot on the least busy of 1/6/11 if known).
    pub fn set_mode(&mut self, m: Mode, best_channel: Option<u32>) -> Option<String> {
        if self.job.is_some() {
            return Some("Busy, wait...".into());
        }
        if self.status.as_ref().is_some_and(|s| s.mode == m) {
            return Some(format!("Already {}", m.label()));
        }
        if m == Mode::Hotspot {
            self.home_ssid = self.status.as_ref().and_then(|s| s.link.as_ref().map(|l| l.ssid.clone()));
        }
        let what = match m {
            Mode::Off => "Turning Wi-Fi off...",
            Mode::Client => "Joining saved networks...",
            Mode::Hotspot => "Starting hotspot...",
        };
        self.job = Some(Job::start(what, move || wifi::set_mode(m, best_channel)));
        None
    }

    /// Details page OK: join this network.
    pub fn join(&mut self) -> Option<String> {
        if self.job.is_some() {
            return Some("Busy, wait...".into());
        }
        let d = self.detail.clone();
        if d.connected {
            return Some("Already connected".into());
        }
        if let Some(uuid) = d.saved_uuid {
            let name = d.ssid.clone();
            self.job = Some(Job::start(format!("Joining {name}..."), move || wifi::connect(&uuid).map(|_| format!("Connected to {name}"))));
            return None;
        }
        if d.ssid.is_empty() {
            return Some("Hidden network: add it with zpod-wifi.txt".into());
        }
        if d.security == "Open" || d.security.is_empty() {
            let name = d.ssid.clone();
            self.job = Some(Job::start(format!("Joining {name}..."), move || wifi::join_open(&name).map(|_| format!("Joined {name} (open)"))));
            return None;
        }
        // secured, not saved: type the password (USB keyboard) on the next page
        self.want_password = true;
        None
    }

    /// Password page: join `ssid` with `pass` (moved in, wiped after use).
    pub fn join_password(&mut self, ssid: String, pass: String) -> Option<String> {
        if self.job.is_some() {
            return Some("Busy, wait...".into());
        }
        let what = format!("Joining {ssid}...");
        self.job = Some(Job::start(what, move || {
            let mut pass = pass;
            let r = zpod_hal::wifijoin::join_with_password(&ssid, &pass);
            // overwrite the passphrase before the String is freed
            let n = pass.len();
            pass.clear();
            pass.extend(std::iter::repeat_n('\0', n));
            drop(pass);
            r
        }));
        None
    }

    /// Saved list Tri: first press arms, second (same row, within 4 s) forgets.
    pub fn forget(&mut self, i: usize) -> Option<String> {
        let s = self.saved.get(i)?.clone();
        match self.forget_armed {
            Some((j, t)) if j == i && t.elapsed() < Duration::from_secs(4) => {
                self.forget_armed = None;
                if self.job.is_some() {
                    return Some("Busy, wait...".into());
                }
                let name = s.name.clone();
                self.job = Some(Job::start(format!("Forgetting {name}..."), move || wifi::delete_saved(&s.uuid).map(|_| format!("Forgot {name}"))));
                None
            }
            _ => {
                self.forget_armed = Some((i, Instant::now()));
                Some("Tri again = forget".to_string())
            }
        }
    }

    pub fn refresh_saved(&mut self) {
        match wifi::saved_networks() {
            Ok(v) => {
                self.saved = v;
                self.saved_err = None;
            }
            Err(e) => self.saved_err = Some(e),
        }
    }

    pub fn refresh_hotspot(&mut self) {
        self.hotspot = wifi::hotspot();
    }

    pub fn busy(&self) -> bool {
        self.job.is_some()
    }

    pub fn new_password(&mut self) {
        if self.job.is_none() && self.hotspot.is_some() {
            self.job = Some(Job::start("New password...", || wifi::hotspot_new_passphrase().map(|_| "New password set".into())));
        }
    }

    /// Finished job -> toast text.
    pub fn poll_job(&mut self) -> Option<String> {
        let r = self.job.as_ref()?.poll()?;
        self.job = None;
        self.refresh_hotspot();
        self.refresh_saved();
        self.refresh_status(true);
        Some(match r {
            Ok(m) => m,
            Err(e) => format!("Failed: {}", e.lines().last().unwrap_or("error")),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_rules() {
        let mut w = WifiTools { detail: NetDetail { ssid: "Cafe".into(), security: "WPA2".into(), saved_uuid: None, connected: false }, ..Default::default() };
        assert_eq!(w.join(), None, "secured + unsaved: ask for the password");
        assert!(w.want_password);
        assert!(w.job.is_none());
        w.detail.connected = true;
        assert_eq!(w.join().as_deref(), Some("Already connected"));
        w.detail = NetDetail { ssid: "".into(), security: "Open".into(), ..Default::default() };
        assert!(w.join().unwrap().contains("Hidden"));
    }

    #[test]
    fn forget_needs_two_presses() {
        let mut w = WifiTools { saved: vec![Saved { name: "HomeNet".into(), uuid: "u1".into(), ..Default::default() }, Saved { name: "Cafe".into(), uuid: "u2".into(), ..Default::default() }], ..Default::default() };
        assert!(w.forget(0).unwrap().contains("Tri again"));
        assert_eq!(w.forget_armed.map(|a| a.0), Some(0));
        assert!(w.forget(1).unwrap().contains("Tri again"), "other row re-arms");
        assert_eq!(w.forget_armed.map(|a| a.0), Some(1));
        assert!(w.job.is_none());
        assert_eq!(w.forget(9), None);
    }
}
