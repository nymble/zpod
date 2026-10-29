//! Tools > Wi-Fi scanner and Bluetooth scanner (M5).
//!
//! Scans run on a background thread so the UI keeps drawing (a Wi-Fi scan
//! takes 3-5 s, Bluetooth discovery about 10 s). The launcher is root, which
//! is what lets it trigger scans; see zpod_hal::scan for the fallbacks.
//! Bindings: Up/Down move, OK/Right/Circle = details, Square = rescan,
//! Triangle = sort, Left/X = back, Home = top.

use std::sync::mpsc::{channel, Receiver};
use std::time::Instant;

use zpod_hal::scan::{self, BtDev, WifiNet, WifiSort};

pub type ScanResult<T> = Result<(Vec<T>, &'static str), String>;

pub struct Scanner<T> {
    pub items: Vec<T>,
    pub busy: Option<Receiver<ScanResult<T>>>,
    pub started: Option<Instant>,
    pub done_at: Option<Instant>,
    pub err: Option<String>,
    pub note: &'static str,
}

impl<T> Default for Scanner<T> {
    fn default() -> Self {
        Scanner { items: Vec::new(), busy: None, started: None, done_at: None, err: None, note: "" }
    }
}

impl<T: Send + 'static> Scanner<T> {
    pub fn start(&mut self, f: fn() -> ScanResult<T>) {
        if self.busy.is_some() {
            return;
        }
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(f());
        });
        self.busy = Some(rx);
        self.started = Some(Instant::now());
    }

    /// Start a scan unless results are fresh (< 60 s).
    pub fn start_if_stale(&mut self, f: fn() -> ScanResult<T>) {
        if self.done_at.map(|t| t.elapsed().as_secs() >= 60).unwrap_or(true) {
            self.start(f);
        }
    }

    /// Collect a finished scan. Returns true when something changed.
    pub fn poll(&mut self) -> bool {
        let Some(rx) = &self.busy else { return false };
        match rx.try_recv() {
            Ok(r) => {
                self.busy = None;
                self.done_at = Some(Instant::now());
                match r {
                    Ok((v, note)) => {
                        self.items = v;
                        self.note = note;
                        self.err = None;
                    }
                    Err(e) => self.err = Some(e),
                }
                true
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => false,
            Err(_) => {
                self.busy = None;
                self.err = Some("scan thread failed".into());
                true
            }
        }
    }

    pub fn age_text(&self) -> String {
        match (self.busy.is_some(), self.done_at) {
            (true, _) => "scanning...".into(),
            (false, Some(t)) => {
                let s = t.elapsed().as_secs();
                if s < 60 {
                    format!("{s} s ago")
                } else {
                    format!("{} min ago", s / 60)
                }
            }
            _ => String::new(),
        }
    }
}

pub fn wifi_job() -> ScanResult<WifiNet> {
    scan::wifi_scan()
}

pub fn bt_job() -> ScanResult<BtDev> {
    scan::bt_scan(10)
}

pub struct Tools {
    pub wifi: Scanner<WifiNet>,
    pub wifi_sort: WifiSort,
    pub bt: Scanner<BtDev>,
    pub bt_by_name: bool,
    /// Rows of the details page: (label, value).
    pub detail: Vec<(&'static str, String)>,
    pub detail_title: String,
}

impl Default for Tools {
    fn default() -> Self {
        Tools { wifi: Scanner::default(), wifi_sort: WifiSort::Signal, bt: Scanner::default(), bt_by_name: false, detail: Vec::new(), detail_title: String::new() }
    }
}

/// Signal strength as 0-4 bars.
pub fn bars(dbm: f32) -> u8 {
    match dbm as i32 {
        d if d >= -55 => 4,
        d if d >= -65 => 3,
        d if d >= -75 => 2,
        d if d >= -85 => 1,
        _ => 0,
    }
}

impl Tools {
    pub fn poll(&mut self) -> bool {
        let a = self.wifi.poll();
        if a {
            scan::sort_wifi(&mut self.wifi.items, self.wifi_sort);
        }
        let b = self.bt.poll();
        if b {
            scan::sort_bt(&mut self.bt.items, self.bt_by_name);
        }
        a || b
    }

    pub fn busy(&self) -> bool {
        self.wifi.busy.is_some() || self.bt.busy.is_some()
    }

    pub fn cycle_wifi_sort(&mut self) {
        self.wifi_sort = self.wifi_sort.next();
        scan::sort_wifi(&mut self.wifi.items, self.wifi_sort);
    }

    pub fn cycle_bt_sort(&mut self) {
        self.bt_by_name = !self.bt_by_name;
        scan::sort_bt(&mut self.bt.items, self.bt_by_name);
    }

    /// Fill `detail` for Wi-Fi row `i`. Returns false if there is no row.
    pub fn wifi_detail(&mut self, i: usize) -> bool {
        let Some(n) = self.wifi.items.get(i) else { return false };
        let band = if n.freq_mhz >= 5955 {
            "6 GHz"
        } else if n.freq_mhz >= 5000 {
            "5 GHz"
        } else {
            "2.4 GHz"
        };
        self.detail_title = if n.ssid.is_empty() { "(hidden network)".into() } else { n.ssid.clone() };
        self.detail = vec![
            ("SSID", if n.ssid.is_empty() { "(hidden)".into() } else { n.ssid.clone() }),
            ("BSSID", n.bssid.clone()),
            ("Channel", format!("{}  ({} MHz, {band})", n.channel, n.freq_mhz)),
            ("Signal", format!("{:.0} dBm  ({}/4 bars)", n.signal_dbm, bars(n.signal_dbm))),
            ("Security", n.security.clone()),
            ("Status", if n.associated { "connected (this ZPOD)".into() } else { "not connected".into() }),
            ("Source", self.wifi.note.to_string()),
        ];
        true
    }

    pub fn bt_detail(&mut self, i: usize) -> bool {
        let Some(d) = self.bt.items.get(i) else { return false };
        self.detail_title = if d.name.is_empty() { d.addr.clone() } else { d.name.clone() };
        let kind = match d.kind.as_str() {
            "BR/EDR" => "Classic (BR/EDR)",
            "LE public" => "Low Energy, public address",
            "LE random" => "Low Energy, random address",
            "" => "unknown",
            k => k,
        };
        self.detail = vec![
            ("Name", if d.name.is_empty() { "(no name advertised)".into() } else { d.name.clone() }),
            ("Address", d.addr.clone()),
            ("Type", kind.into()),
            ("RSSI", d.rssi.map(|r| format!("{r} dBm  ({}/4 bars)", bars(r as f32))).unwrap_or_else(|| "--".into())),
            ("Source", self.bt.note.to_string()),
        ];
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait<T: Send + 'static>(s: &mut Scanner<T>) {
        for _ in 0..200 {
            if s.poll() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("scan did not finish");
    }

    fn fake_wifi() -> ScanResult<WifiNet> {
        let n = |s: &str, d: f32, ch: u32| WifiNet { ssid: s.into(), signal_dbm: d, channel: ch, freq_mhz: 2407 + 5 * ch, security: "WPA2".into(), ..Default::default() };
        Ok((vec![n("b", -70.0, 6), n("a", -50.0, 11), n("", -90.0, 1)], "fake"))
    }

    fn failing() -> ScanResult<BtDev> {
        Err("NotReady".into())
    }

    #[test]
    fn scanner_runs_in_background_and_sorts() {
        let mut t = Tools::default();
        t.wifi.start(fake_wifi);
        assert!(t.busy());
        assert_eq!(t.wifi.age_text(), "scanning...");
        for _ in 0..200 {
            if t.poll() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(!t.busy());
        assert_eq!(t.wifi.items[0].ssid, "a"); // strongest first
        t.cycle_wifi_sort();
        assert_eq!(t.wifi_sort, WifiSort::Name);
        assert_eq!(t.wifi.items.iter().map(|n| n.ssid.as_str()).collect::<Vec<_>>(), ["a", "b", ""]);
        t.cycle_wifi_sort();
        assert_eq!(t.wifi.items[0].channel, 1);
        assert!(t.wifi_detail(2));
        assert_eq!(t.detail_title, "a");
        assert!(t.detail.iter().any(|(k, v)| *k == "Channel" && v.contains("2.4 GHz")));
        assert!(!t.wifi_detail(9));
        // fresh results are not rescanned on re-entry
        t.wifi.start_if_stale(fake_wifi);
        assert!(t.wifi.busy.is_none());
    }

    #[test]
    fn scan_errors_keep_old_results() {
        let mut s: Scanner<BtDev> = Scanner::default();
        s.items.push(BtDev { addr: "AA".into(), ..Default::default() });
        s.start(failing);
        wait(&mut s);
        assert_eq!(s.err.as_deref(), Some("NotReady"));
        assert_eq!(s.items.len(), 1);
        assert_eq!(bars(-50.0), 4);
        assert_eq!(bars(-95.0), 0);
    }
}
