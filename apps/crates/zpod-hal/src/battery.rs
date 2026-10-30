//! Battery history (M6): the `zpod-battery` logger writes one line a minute
//! to `/data/zpod/battery.log`; the launcher's Battery page reads it back.
//!
//! Line format (space separated, `#` comments ignored):
//!
//! ```text
//! <unix time> <SOC %> <VCELL V> <CRATE %/h> <charging 0|1>
//! 1793380000 87.42 3.981 -6.24 0
//! ```
//!
//! CRATE is the MAX17048 register 0x16 (signed, 0.208 %/h per LSB; positive
//! = charging). The log is capped: when it passes [`LOG_CAP`] bytes it is
//! renamed to `battery.log.1` (one old file kept), so at most ~2 x 11 days.
//! Finished discharge sessions (unplugged -> next charge) are also appended
//! to `battery-sessions.log` (`<start> <end> <start SOC> <end SOC>`, capped
//! at [`SESSIONS_KEEP`] lines) so "typical runtime" outlives the rotation.
//!
//! Everything below is pure and unit-tested; the estimates degrade to
//! `None` instead of guessing when there is too little history.

use std::path::PathBuf;

pub const LOG_CAP: u64 = 512 * 1024;
pub const SESSIONS_KEEP: usize = 60;
/// CRATE above this (%/h) counts as charging (same rule as the status bar).
pub const CHARGING_RATE: f32 = 0.5;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub t: i64,
    pub soc: f32,
    pub volts: f32,
    pub rate: f32,
    pub charging: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Session {
    pub start: i64,
    pub end: i64,
    pub soc0: f32,
    pub soc1: f32,
}

impl Session {
    /// Runtime extrapolated to a full 100 % -> 0 % discharge, in seconds.
    pub fn full_runtime(&self) -> Option<i64> {
        let used = self.soc0 - self.soc1;
        let secs = self.end - self.start;
        (used >= 15.0 && secs >= 1800).then(|| (secs as f32 * 100.0 / used) as i64)
    }
}

pub fn dir() -> PathBuf {
    std::env::var_os("ZPOD_BATTERY_DIR").map(Into::into).unwrap_or_else(|| "/data/zpod".into())
}
pub fn log_path() -> PathBuf {
    dir().join("battery.log")
}
pub fn sessions_path() -> PathBuf {
    dir().join("battery-sessions.log")
}

pub fn format_sample(s: &Sample) -> String {
    format!("{} {:.2} {:.3} {:.2} {}\n", s.t, s.soc, s.volts, s.rate, s.charging as u8)
}

pub fn parse_log(text: &str) -> Vec<Sample> {
    let mut v: Vec<Sample> = text
        .lines()
        .filter_map(|l| {
            let l = l.trim();
            if l.starts_with('#') {
                return None;
            }
            let mut it = l.split_whitespace();
            let t = it.next()?.parse().ok()?;
            let soc: f32 = it.next()?.parse().ok()?;
            let volts = it.next()?.parse().ok()?;
            let rate = it.next()?.parse().ok()?;
            let charging = it.next()? == "1";
            (soc.is_finite() && (0.0..=101.0).contains(&soc)).then_some(Sample { t, soc, volts, rate, charging })
        })
        .collect();
    // a clock jump (RTC unset at boot) can reorder lines
    v.sort_by_key(|s| s.t);
    v
}

pub fn parse_sessions(text: &str) -> Vec<Session> {
    text.lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            Some(Session { start: it.next()?.parse().ok()?, end: it.next()?.parse().ok()?, soc0: it.next()?.parse().ok()?, soc1: it.next()?.parse().ok()? })
        })
        .collect()
}

pub fn format_session(s: &Session) -> String {
    format!("{} {} {:.1} {:.1}\n", s.start, s.end, s.soc0, s.soc1)
}

/// Both log files (old first), parsed.
pub fn load() -> (Vec<Sample>, Vec<Session>) {
    if crate::sys::sim() {
        if let Some(p) = std::env::var_os("ZPOD_SIM_BATTLOG") {
            let t = std::fs::read_to_string(p).unwrap_or_default();
            let s = std::env::var_os("ZPOD_SIM_BATTSESS").and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
            return (parse_log(&t), parse_sessions(&s));
        }
    }
    let mut text = std::fs::read_to_string(log_path().with_extension("log.1")).unwrap_or_default();
    text.push_str(&std::fs::read_to_string(log_path()).unwrap_or_default());
    (parse_log(&text), parse_sessions(&std::fs::read_to_string(sessions_path()).unwrap_or_default()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Charging,
    /// On the charger at (almost) 100 %, no current flowing.
    Full,
    Discharging,
}

pub fn state(soc: f32, rate: f32) -> State {
    if rate > CHARGING_RATE {
        State::Charging
    } else if soc >= 99.0 && rate > -0.6 {
        State::Full
    } else {
        State::Discharging
    }
}

/// Index of the first sample of the current discharge (after the last
/// sample that was charging). None if the whole log is one discharge.
fn discharge_start(s: &[Sample]) -> usize {
    s.iter().rposition(|x| x.charging).map(|i| i + 1).unwrap_or(0)
}

/// When the cell was last taken off the charger (end of the last charge),
/// and the SOC it reached. None if the log has no charge yet.
pub fn last_charge(s: &[Sample]) -> Option<(i64, f32)> {
    let i = s.iter().rposition(|x| x.charging)?;
    Some((s[i].t, s[i].soc))
}

/// Discharge curve since the last charge: (t, soc) points, thinned to at
/// most `max` points (for the chart). Gaps over 10 min (powered off) are
/// kept as they are; the chart just draws a straight segment.
pub fn discharge_curve(s: &[Sample], max: usize) -> Vec<(i64, f32)> {
    let d = &s[discharge_start(s)..];
    if d.is_empty() || max == 0 {
        return Vec::new();
    }
    let step = d.len().div_ceil(max).max(1);
    let mut v: Vec<(i64, f32)> = d.iter().step_by(step).map(|x| (x.t, x.soc)).collect();
    let last = d[d.len() - 1];
    if v.last().map(|p| p.0) != Some(last.t) {
        v.push((last.t, last.soc));
    }
    v
}

/// Least-squares slope of SOC over the last `window` seconds (%/h), from
/// samples that are not charging. None with < 10 min or < 5 samples.
pub fn trend(s: &[Sample], now: i64, window: i64) -> Option<f32> {
    let pts: Vec<&Sample> = s.iter().filter(|x| x.t >= now - window && x.t <= now && !x.charging).collect();
    if pts.len() < 5 || pts[pts.len() - 1].t - pts[0].t < 600 {
        return None;
    }
    let n = pts.len() as f64;
    let t0 = pts[0].t as f64;
    let (mut sx, mut sy, mut sxx, mut sxy) = (0.0, 0.0, 0.0, 0.0);
    for p in &pts {
        let x = (p.t as f64 - t0) / 3600.0;
        let y = p.soc as f64;
        sx += x;
        sy += y;
        sxx += x * x;
        sxy += x * y;
    }
    let den = n * sxx - sx * sx;
    (den.abs() > 1e-9).then(|| ((n * sxy - sx * sy) / den) as f32)
}

/// Smoothed discharge rate (%/h, positive number) from CRATE readings and
/// the SOC trend of the last 30 min. CRATE is noisy minute to minute; the
/// trend is steady but lags, so use their mean when both exist.
pub fn discharge_rate(s: &[Sample], now: i64, crate_now: f32) -> Option<f32> {
    let recent: Vec<f32> = s.iter().rev().take_while(|x| x.t >= now - 900).filter(|x| !x.charging).map(|x| x.rate).collect();
    let mut c = if recent.is_empty() { crate_now } else { recent.iter().sum::<f32>() / recent.len() as f32 };
    if crate_now < 0.0 && !recent.is_empty() {
        c = 0.7 * c + 0.3 * crate_now;
    }
    let c = (-c).max(0.0);
    let t = trend(s, now, 1800).map(|r| (-r).max(0.0));
    let r = match (c > 0.05, t) {
        (true, Some(t)) if t > 0.05 => (c + t) / 2.0,
        (true, _) => c,
        (false, Some(t)) if t > 0.05 => t,
        _ => return None,
    };
    (r > 0.05).then_some(r)
}

/// Seconds left: to empty when discharging, to full when charging.
pub fn time_left(s: &[Sample], now: i64, soc: f32, crate_now: f32) -> Option<i64> {
    match state(soc, crate_now) {
        State::Full => None,
        State::Charging => {
            let r: Vec<f32> = s.iter().rev().take_while(|x| x.t >= now - 900).filter(|x| x.charging).map(|x| x.rate).collect();
            let rate = if r.is_empty() { crate_now } else { (r.iter().sum::<f32>() / r.len() as f32 + crate_now) / 2.0 };
            (rate > CHARGING_RATE).then(|| (((100.0 - soc).max(0.0) / rate) * 3600.0) as i64)
        }
        State::Discharging => discharge_rate(s, now, crate_now).map(|r| ((soc.max(0.0) / r) * 3600.0) as i64).filter(|t| *t < 14 * 86400),
    }
}

/// Typical runtime from 100 % to 0 % (median of the finished sessions,
/// plus the current one if it is long enough).
pub fn typical_runtime(sessions: &[Session], current: Option<Session>) -> Option<(i64, usize)> {
    let mut v: Vec<i64> = sessions.iter().chain(current.iter()).filter_map(|s| s.full_runtime()).collect();
    if v.is_empty() {
        return None;
    }
    v.sort_unstable();
    Some((v[v.len() / 2], v.len()))
}

/// The discharge in progress as a session (for typical_runtime).
pub fn current_session(s: &[Sample]) -> Option<Session> {
    let d = &s[discharge_start(s)..];
    let (a, b) = (d.first()?, d.last()?);
    Some(Session { start: a.t, end: b.t, soc0: a.soc, soc1: b.soc })
}

/// Logger step: given the previous sample and the new one, the finished
/// session to record when a charge starts after a discharge.
pub fn session_ended(prev_start: Option<(i64, f32)>, prev: Option<&Sample>, now: &Sample) -> Option<Session> {
    let (st, s0) = prev_start?;
    let p = prev?;
    (now.charging && !p.charging).then_some(Session { start: st, end: p.t, soc0: s0, soc1: p.soc })
}

/// "3 h 20 min", "45 min", "2 d 4 h".
pub fn human_duration(secs: i64) -> String {
    let m = (secs.max(0) + 30) / 60;
    let (d, h, mm) = (m / 1440, m / 60 % 24, m % 60);
    if d > 0 {
        format!("{d} d {h} h")
    } else if h > 0 {
        format!("{h} h {mm:02} min")
    } else {
        format!("{mm} min")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(t: i64, soc: f32, rate: f32, charging: bool) -> Sample {
        Sample { t, soc, volts: 3.9, rate, charging }
    }

    /// 2 h of charging to 100 %, then a 10 %/h discharge for `mins` minutes.
    fn history(mins: i64) -> Vec<Sample> {
        let mut v = Vec::new();
        let t0 = 1_793_000_000;
        for m in 0..120 {
            v.push(s(t0 + m * 60, 60.0 + m as f32 / 3.0, 20.0, true));
        }
        let t1 = t0 + 120 * 60;
        for m in 0..mins {
            v.push(s(t1 + m * 60, 100.0 - m as f32 / 6.0, -10.0, false));
        }
        v
    }

    #[test]
    fn round_trip_and_bad_lines() {
        let a = s(1_793_000_000, 87.42, -6.24, false);
        let txt = format!("# zpod battery log\n{}garbage\n1 2 3\n{}", format_sample(&a), format_sample(&s(1_792_999_000, 90.0, 1.0, true)));
        let v = parse_log(&txt);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].t, 1_792_999_000, "sorted by time");
        assert!((v[1].soc - 87.42).abs() < 1e-3 && !v[1].charging);
        let ss = Session { start: 1, end: 7201, soc0: 100.0, soc1: 50.0 };
        assert_eq!(parse_sessions(&format_session(&ss)), vec![ss]);
    }

    #[test]
    fn states() {
        assert_eq!(state(50.0, 12.0), State::Charging);
        assert_eq!(state(100.0, 0.0), State::Full);
        assert_eq!(state(99.5, -0.2), State::Full);
        assert_eq!(state(80.0, -5.0), State::Discharging);
        assert_eq!(state(80.0, 0.0), State::Discharging);
    }

    #[test]
    fn last_charge_and_curve() {
        let v = history(180);
        let (t, soc) = last_charge(&v).unwrap();
        assert_eq!(t, 1_793_000_000 + 119 * 60);
        assert!(soc > 99.0);
        let c = discharge_curve(&v, 50);
        assert!(c.len() <= 51 && c.len() >= 40, "{}", c.len());
        assert_eq!(c[0].1, 100.0);
        assert_eq!(c.last().unwrap().0, v.last().unwrap().t, "ends at the newest sample");
        assert!(discharge_curve(&[], 10).is_empty());
        assert_eq!(last_charge(&history(0)[..0]), None);
    }

    #[test]
    fn time_left_uses_smoothed_rate() {
        let v = history(180); // 30 % used in 3 h -> 10 %/h, 70 % left -> 7 h
        let now = v.last().unwrap().t;
        let soc = v.last().unwrap().soc;
        let left = time_left(&v, now, soc, -10.0).unwrap();
        assert!((left - 7 * 3600).abs() < 600, "{left}");
        // one noisy CRATE reading barely moves it
        let left2 = time_left(&v, now, soc, -30.0).unwrap();
        assert!(left2 > 4 * 3600, "{left2}");
        // no history: CRATE alone
        assert_eq!(time_left(&[], now, 50.0, -10.0), Some(5 * 3600));
        assert_eq!(time_left(&[], now, 50.0, 0.0), None, "no rate, no guess");
        assert_eq!(time_left(&[], now, 100.0, 0.0), None, "full");
        // charging at 20 %/h from 60 % -> 2 h to full
        assert_eq!(time_left(&[], now, 60.0, 20.0), Some(7200));
    }

    #[test]
    fn typical_runtime_median() {
        let mk = |h: i64, used: f32| Session { start: 0, end: h * 3600, soc0: 100.0, soc1: 100.0 - used };
        let v = [mk(5, 50.0), mk(9, 90.0), mk(2, 40.0), mk(1, 5.0)]; // 10 h, 10 h, 5 h, (too short)
        assert_eq!(typical_runtime(&v, None), Some((10 * 3600, 3)));
        assert_eq!(typical_runtime(&[], None), None);
        let cur = current_session(&history(180)).unwrap();
        assert!((cur.full_runtime().unwrap() - 10 * 3600).abs() < 400);
    }

    #[test]
    fn logger_records_session_on_plug_in() {
        let a = s(1000, 80.0, -8.0, false);
        let b = s(1060, 79.9, 15.0, true);
        assert_eq!(session_ended(Some((100, 100.0)), Some(&a), &b), Some(Session { start: 100, end: 1000, soc0: 100.0, soc1: 80.0 }));
        assert_eq!(session_ended(Some((100, 100.0)), Some(&b), &b), None);
        assert_eq!(session_ended(None, Some(&a), &b), None);
    }

    #[test]
    fn durations() {
        assert_eq!(human_duration(45 * 60), "45 min");
        assert_eq!(human_duration(3 * 3600 + 20 * 60), "3 h 20 min");
        assert_eq!(human_duration(2 * 86400 + 4 * 3600), "2 d 4 h");
    }
}
