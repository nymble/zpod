//! Receiver state: what the GPS screen shows, and which of the §8.4 states
//! applies. Time is passed in (milliseconds on any monotonic clock).

use std::collections::BTreeMap;

use crate::parse::{parse_sentence, NmeaError, Sentence, Utc};

/// A fix older than this is `Stale` (spec: 2-5 s, tune on device; the
/// ND-105C reports at 1 Hz).
pub const STALE_MS: u64 = 3000;
/// No valid sentence for this long = device present but no data.
pub const NO_DATA_MS: u64 = 5000;
/// This many checksum errors within [`ERROR_WINDOW_MS`] with no valid
/// sentence in between = `Error` (wrong baud rate, noise).
pub const ERROR_STORM: usize = 8;
pub const ERROR_WINDOW_MS: u64 = 5000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FixKind {
    None,
    Fix2D,
    Fix3D,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GpsStatus {
    /// USB is in gadget mode, or the image lacks the serial drivers.
    Unsupported(String),
    NoDevice,
    /// Serial port open, no valid NMEA (power, cable, baud, firmware).
    DeviceNoData,
    /// Valid NMEA, searching.
    NoFix,
    Fix2D,
    Fix3D,
    /// Had a fix, nothing new within [`STALE_MS`].
    Stale,
    /// I/O error, unplug while reading, checksum storm.
    Error(String),
}

impl GpsStatus {
    pub fn label(&self) -> &str {
        match self {
            GpsStatus::Unsupported(_) => "Unsupported",
            GpsStatus::NoDevice => "No GPS device",
            GpsStatus::DeviceNoData => "No data",
            GpsStatus::NoFix => "Searching",
            GpsStatus::Fix2D => "2D fix",
            GpsStatus::Fix3D => "3D fix",
            GpsStatus::Stale => "Stale fix",
            GpsStatus::Error(_) => "Error",
        }
    }
    pub fn has_fix(&self) -> bool {
        matches!(self, GpsStatus::Fix2D | GpsStatus::Fix3D)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sat {
    /// talker ("GP", "GL", "GA", "BD"/"GB", "GN") + PRN
    pub talker: [u8; 2],
    pub prn: u16,
    pub elev: Option<u8>,
    pub az: Option<u16>,
    pub snr: Option<u8>,
    pub used: bool,
}

#[derive(Clone, Debug, Default)]
pub struct GpsState {
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub alt_m: Option<f32>,
    pub speed_kn: Option<f32>,
    pub time: Option<Utc>,
    pub date: Option<(u8, u8, u16)>,
    pub hdop: Option<f32>,
    /// from GGA (or the GSA PRN count when GGA has none)
    pub sats_used: Option<u8>,
    /// GSA mode (1/2/3), None until a GSA arrives
    gsa_mode: Option<u8>,
    gsa_used: BTreeMap<[u8; 2], Vec<u16>>,
    /// complete GSV cycles per talker
    views: BTreeMap<[u8; 2], Vec<crate::parse::GsvSat>>,
    in_view: BTreeMap<[u8; 2], u8>,
    pending: BTreeMap<[u8; 2], Vec<crate::parse::GsvSat>>,
    /// currently fixed (latest GGA/RMC says valid)
    fixed: bool,
    pub last_valid_ms: Option<u64>,
    pub last_fix_ms: Option<u64>,
    /// recent checksum / framing errors (timestamps)
    errors: Vec<u64>,
    pub sentences: u64,
    pub bad_sentences: u64,
}

fn talker(line: &str) -> [u8; 2] {
    let b = line.trim_start_matches(['$', '!']).as_bytes();
    if b.len() >= 2 {
        [b[0], b[1]]
    } else {
        *b"??"
    }
}

impl GpsState {
    /// Feed one line; returns what it was (errors are counted, not fatal).
    pub fn feed(&mut self, line: &str, now_ms: u64) -> Result<(), NmeaError> {
        let line = line.trim();
        if line.is_empty() {
            return Ok(());
        }
        let t = talker(line);
        match parse_sentence(line) {
            Ok(s) => {
                self.sentences += 1;
                self.last_valid_ms = Some(now_ms);
                self.errors.clear();
                self.apply(t, s, now_ms);
                Ok(())
            }
            Err(NmeaError::Unsupported) => {
                // valid checksum, type we do not need: still proof of life
                self.sentences += 1;
                self.last_valid_ms = Some(now_ms);
                self.errors.clear();
                Err(NmeaError::Unsupported)
            }
            Err(e) => {
                self.bad_sentences += 1;
                self.errors.retain(|&x| now_ms.saturating_sub(x) < ERROR_WINDOW_MS);
                self.errors.push(now_ms);
                Err(e)
            }
        }
    }

    fn apply(&mut self, t: [u8; 2], s: Sentence, now_ms: u64) {
        match s {
            Sentence::Gga(g) => {
                if g.time.is_some() {
                    self.time = g.time;
                }
                self.sats_used = g.sats_used.or(self.sats_used);
                self.hdop = g.hdop.or(self.hdop);
                if g.quality > 0 && g.lat.is_some() && g.lon.is_some() {
                    self.lat = g.lat;
                    self.lon = g.lon;
                    self.alt_m = g.alt_m;
                    self.fixed = true;
                    self.last_fix_ms = Some(now_ms);
                } else {
                    self.fixed = false;
                }
            }
            Sentence::Rmc(r) => {
                if r.time.is_some() {
                    self.time = r.time;
                }
                if r.date.is_some() {
                    self.date = r.date;
                }
                if r.valid && r.lat.is_some() && r.lon.is_some() {
                    self.lat = r.lat;
                    self.lon = r.lon;
                    self.speed_kn = r.speed_kn;
                    self.fixed = true;
                    self.last_fix_ms = Some(now_ms);
                } else {
                    self.fixed = false;
                }
            }
            Sentence::Gsa(a) => {
                self.gsa_mode = Some(a.mode);
                self.hdop = a.hdop.or(self.hdop);
                // GNGSA: one per constellation, all "GN": accumulate per cycle
                // by keying on the first PRN's range is overkill; keep the last
                // per talker, and for GN merge within one second.
                let e = self.gsa_used.entry(t).or_default();
                if t == *b"GN" && self.last_fix_ms == Some(now_ms) {
                    e.extend(a.prns);
                } else {
                    *e = a.prns;
                }
                if a.mode <= 1 {
                    self.fixed = false;
                }
            }
            Sentence::Gsv(v) => {
                self.in_view.insert(t, v.in_view);
                let p = self.pending.entry(t).or_default();
                if v.num == 1 {
                    p.clear();
                }
                p.extend(v.sats);
                if v.num == v.total {
                    let done = std::mem::take(p);
                    self.views.insert(t, done);
                }
                if v.in_view == 0 {
                    self.views.insert(t, vec![]);
                }
            }
        }
    }

    pub fn fix_kind(&self) -> FixKind {
        if !self.fixed {
            return FixKind::None;
        }
        match self.gsa_mode {
            Some(3) => FixKind::Fix3D,
            Some(2) => FixKind::Fix2D,
            Some(_) => FixKind::None,
            None => FixKind::Fix2D, // GGA/RMC only: at least 2D
        }
    }

    pub fn sats_in_view(&self) -> u16 {
        self.in_view.values().map(|&n| n as u16).sum()
    }

    /// Satellites from the last complete GSV cycles, `used` from GSA.
    pub fn sats(&self) -> Vec<Sat> {
        let used: Vec<u16> = self.gsa_used.values().flatten().copied().collect();
        let mut v: Vec<Sat> = self
            .views
            .iter()
            .flat_map(|(t, s)| s.iter().map(move |x| (t, x)))
            .map(|(t, x)| Sat { talker: *t, prn: x.prn, elev: x.elev, az: x.az, snr: x.snr, used: used.contains(&x.prn) })
            .collect();
        v.sort_by_key(|s| (std::cmp::Reverse(s.snr.unwrap_or(0)), s.prn));
        v
    }

    /// Satellites used: GGA's count, else the GSA PRN list.
    pub fn used_count(&self) -> Option<u8> {
        self.sats_used.or_else(|| {
            let n: usize = self.gsa_used.values().map(|v| v.len()).sum();
            (self.gsa_mode.is_some()).then_some(n as u8)
        })
    }

    /// The §8.4 state. `host_ok` false -> Unsupported (reason given);
    /// `device` = a serial port is open; `io_error` = last read failed.
    pub fn status(&self, now_ms: u64, device: bool, io_error: Option<&str>) -> GpsStatus {
        if !device {
            return GpsStatus::NoDevice;
        }
        if let Some(e) = io_error {
            return GpsStatus::Error(e.to_string());
        }
        let recent_errors = self.errors.iter().filter(|&&x| now_ms.saturating_sub(x) < ERROR_WINDOW_MS).count();
        let valid_age = self.last_valid_ms.map(|t| now_ms.saturating_sub(t));
        if recent_errors >= ERROR_STORM && valid_age.is_none_or(|a| a > 2000) {
            return GpsStatus::Error(format!("{recent_errors} bad sentences (baud rate? noise?)"));
        }
        let fix_age = self.last_fix_ms.map(|t| now_ms.saturating_sub(t));
        match valid_age {
            None => return GpsStatus::DeviceNoData,
            Some(a) if a > NO_DATA_MS => {
                return if fix_age.is_some() { GpsStatus::Stale } else { GpsStatus::DeviceNoData };
            }
            _ => {}
        }
        if self.fixed {
            if fix_age.is_some_and(|a| a > STALE_MS) {
                return GpsStatus::Stale;
            }
            return match self.fix_kind() {
                FixKind::Fix3D => GpsStatus::Fix3D,
                FixKind::Fix2D => GpsStatus::Fix2D,
                FixKind::None => GpsStatus::NoFix,
            };
        }
        GpsStatus::NoFix
    }

    /// Forget everything (device unplugged / reopened).
    pub fn reset(&mut self) {
        *self = GpsState::default();
    }
}

/// Decimal degrees -> ("37.774929° N", "122.419416° W").
pub fn format_dd(lat: f64, lon: f64) -> (String, String) {
    (format!("{:.6}\u{b0} {}", lat.abs(), if lat < 0.0 { 'S' } else { 'N' }), format!("{:.6}\u{b0} {}", lon.abs(), if lon < 0.0 { 'W' } else { 'E' }))
}

/// Degrees + decimal minutes: ("37\u{b0} 46.4957' N", ...).
pub fn format_dmm(lat: f64, lon: f64) -> (String, String) {
    let f = |v: f64, pos: char, neg: char| {
        let a = v.abs();
        let d = a.trunc();
        format!("{}\u{b0} {:07.4}' {}", d as u32, (a - d) * 60.0, if v < 0.0 { neg } else { pos })
    };
    (f(lat, 'N', 'S'), f(lon, 'E', 'W'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(payload: &str) -> String {
        let cs = payload.bytes().fold(0u8, |a, b| a ^ b);
        format!("${payload}*{cs:02X}")
    }

    #[test]
    fn states_from_fixture_cycle() {
        let mut s = GpsState::default();
        assert_eq!(s.status(0, false, None), GpsStatus::NoDevice);
        assert_eq!(s.status(0, true, None), GpsStatus::DeviceNoData);
        // searching
        s.feed(&line("GPGGA,000012.000,,,,,0,00,,,M,,M,,"), 1000).unwrap();
        s.feed(&line("GPGSA,A,1,,,,,,,,,,,,,,,"), 1000).unwrap();
        assert_eq!(s.status(1000, true, None), GpsStatus::NoFix);
        // 3D fix
        s.feed(&line("GPGGA,123519,4807.038,N,01131.000,E,1,08,0.9,545.4,M,46.9,M,,"), 2000).unwrap();
        s.feed(&line("GPGSA,A,3,04,05,,09,12,,,24,,,,,2.5,1.3,2.1"), 2000).unwrap();
        assert_eq!(s.status(2000, true, None), GpsStatus::Fix3D);
        assert_eq!(s.used_count(), Some(8));
        // 2D
        s.feed(&line("GPGSA,A,2,04,05,09,,,,,,,,,,3.5,2.3,2.1"), 2100).unwrap();
        assert_eq!(s.status(2100, true, None), GpsStatus::Fix2D);
        // stale: other sentences keep coming, no new fix
        s.feed(&line("GPVTG,054.7,T,034.4,M,005.5,N,010.2,K"), 5500).unwrap_err();
        assert_eq!(s.status(5500, true, None), GpsStatus::Stale);
        // data stops entirely after a fix: still stale (position kept)
        assert_eq!(s.status(20_000, true, None), GpsStatus::Stale);
        assert!(s.lat.is_some());
        // fix lost explicitly
        s.feed(&line("GPRMC,123600,V,,,,,,,230394,,"), 20_100).unwrap();
        assert_eq!(s.status(20_100, true, None), GpsStatus::NoFix);
        // I/O error wins
        assert!(matches!(s.status(20_100, true, Some("read: EIO")), GpsStatus::Error(_)));
    }

    #[test]
    fn checksum_storm_is_an_error_but_one_bad_line_is_not() {
        let mut s = GpsState::default();
        assert!(s.feed("$GPGGA,1,2,3*00", 0).is_err());
        assert_eq!(s.status(0, true, None), GpsStatus::DeviceNoData);
        for i in 0..ERROR_STORM as u64 {
            let _ = s.feed("$GPGGA,garbage*00", 100 + i);
        }
        assert!(matches!(s.status(200, true, None), GpsStatus::Error(_)));
        // a valid sentence clears it
        s.feed(&line("GPGGA,000012.000,,,,,0,00,,,M,,M,,"), 300).unwrap();
        assert_eq!(s.status(300, true, None), GpsStatus::NoFix);
        assert_eq!(s.bad_sentences, 1 + ERROR_STORM as u64);
    }

    #[test]
    fn gsv_cycle_assembles_and_marks_used() {
        let mut s = GpsState::default();
        for l in [
            "GPGSV,2,1,07,04,77,036,42,05,45,145,38,09,30,250,,12,10,320,21",
            "GPGSV,2,2,07,24,60,100,45,25,05,010,,29,15,200,30",
        ] {
            s.feed(&line(l), 0).unwrap();
        }
        s.feed(&line("GPGSA,A,3,04,05,24,29,,,,,,,,,1.9,1.0,1.6"), 0).unwrap();
        let sats = s.sats();
        assert_eq!(s.sats_in_view(), 7);
        assert_eq!(sats.len(), 7);
        assert_eq!((sats[0].prn, sats[0].snr, sats[0].used), (24, Some(45), true));
        assert_eq!(sats.iter().filter(|x| x.used).count(), 4);
        assert!(sats.iter().any(|x| x.prn == 9 && x.snr.is_none() && !x.used));
        // a half cycle does not replace the last complete one
        s.feed(&line("GPGSV,2,1,07,04,77,036,10,05,45,145,11,09,30,250,,12,10,320,12"), 1).unwrap();
        assert_eq!(s.sats().len(), 7);
        assert_eq!(s.used_count(), Some(4), "GSA count without GGA");
    }

    #[test]
    fn formats() {
        let (a, b) = format_dd(37.774_929, -122.419_416);
        assert_eq!(a, "37.774929\u{b0} N");
        assert_eq!(b, "122.419416\u{b0} W");
        let (a, b) = format_dmm(48.1173, 11.516_666);
        assert_eq!(a, "48\u{b0} 07.0380' N");
        assert_eq!(b, "11\u{b0} 31.0000' E");
    }
}
