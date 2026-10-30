//! Tools > Signal meter, Channels, Saved networks, Hotspot (M6) state.
//! Slow NetworkManager calls (switching, hotspot on/off) run on a thread;
//! the page shows "working..." until they finish.

use std::collections::VecDeque;
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

use zpod_hal::scan::WifiNet;
use zpod_hal::wifi::{self, Hotspot, Link, Saved};

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
    pub bssid: String,
    pub ssid: String,
    pub channel: u32,
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
                    let d = self.meter_scan.items.iter().find(|n| n.bssid == t.bssid).map(|n| n.signal_dbm as i32);
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

    pub fn connect(&mut self, i: usize) -> Option<String> {
        let s = self.saved.get(i)?.clone();
        if self.job.is_some() {
            return Some("Busy, wait...".into());
        }
        let name = s.name.clone();
        self.job = Some(Job::start(format!("Connecting to {name}..."), move || wifi::connect(&s.uuid).map(|_| format!("Connected to {name}"))));
        None
    }

    /// Hotspot on/off (on: on the least busy of 1/6/11 if a scan is known).
    pub fn toggle_hotspot(&mut self, best_channel: Option<u32>) {
        if self.job.is_some() {
            return;
        }
        let on = self.hotspot.as_ref().is_some_and(|h| h.active);
        if on {
            self.job = Some(Job::start("Stopping hotspot...", || wifi::hotspot_off().map(|_| "Hotspot off; rejoining Wi-Fi".into())));
        } else {
            self.home_ssid = wifi::link().map(|l| l.ssid);
            self.job = Some(Job::start("Starting hotspot...", move || wifi::hotspot_on(best_channel).map(|h| format!("Hotspot {} on", h.ssid))));
        }
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
        Some(match r {
            Ok(m) => m,
            Err(e) => format!("Failed: {}", e.lines().last().unwrap_or("error")),
        })
    }
}
