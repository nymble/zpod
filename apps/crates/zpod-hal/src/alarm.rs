//! Alarm and countdown timer shared by the Clock app and the launcher (M5).
//!
//! The Clock app (unprivileged) edits the settings; the launcher (always
//! running, owns the panel) watches them and rings: it stops any running app,
//! shows the ring screen (snooze / dismiss) and plays a beep pattern through
//! the DAC. Both use one small text file, `<data dir>/clock/alarm.conf`
//! (`$ZPOD_DATA_DIR`, on the image `/data/zpod/appdata`), so the settings
//! survive a reboot.
//!
//! Time source: the system clock, which `zpod-rtc` sets from the DS3231 at
//! boot (before the launcher starts) and writes back when NTP is synced, so
//! alarms work without a network. The ZPOD cannot wake itself from power-off
//! (see docs/MUSIC-PLAYER.md / docs/TEST-M5.md): an alarm rings only while
//! the ZPOD is on.

use std::path::PathBuf;

use zpod_core::DateTime;

pub const FILE: &str = "alarm.conf";
/// Snooze lengths offered in the Clock app (minutes).
pub const SNOOZE_CHOICES: [u8; 4] = [5, 9, 10, 15];
/// An alarm minute that was missed by up to this many seconds still rings
/// (the launcher may be busy for a moment, e.g. during a scan).
pub const LATE_GRACE_S: i64 = 120;
/// Stop ringing on its own after this long (counts as dismissed).
pub const RING_MAX_S: u64 = 300;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Repeat {
    /// Ring once, then switch the alarm off.
    Once,
    #[default]
    Daily,
    Weekdays,
    Weekends,
}

impl Repeat {
    pub const ALL: [Repeat; 4] = [Repeat::Daily, Repeat::Weekdays, Repeat::Weekends, Repeat::Once];
    pub fn name(self) -> &'static str {
        match self {
            Repeat::Once => "once",
            Repeat::Daily => "daily",
            Repeat::Weekdays => "weekdays",
            Repeat::Weekends => "weekends",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Repeat::Once => "Once",
            Repeat::Daily => "Every day",
            Repeat::Weekdays => "Mon-Fri",
            Repeat::Weekends => "Sat, Sun",
        }
    }
    pub fn parse(s: &str) -> Self {
        Repeat::ALL.iter().copied().find(|r| r.name() == s.trim()).unwrap_or_default()
    }
    pub fn next(self) -> Self {
        let i = Repeat::ALL.iter().position(|r| *r == self).unwrap_or(0);
        Repeat::ALL[(i + 1) % Repeat::ALL.len()]
    }
    /// Does it ring on this weekday (0 = Sunday)?
    pub fn on_day(self, wd: u8) -> bool {
        match self {
            Repeat::Once | Repeat::Daily => true,
            Repeat::Weekdays => (1..=5).contains(&wd),
            Repeat::Weekends => wd == 0 || wd == 6,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub on: bool,
    pub hour: u8,
    pub minute: u8,
    pub repeat: Repeat,
    pub snooze_min: u8,
    /// Countdown timer end (unix seconds), 0 = not running.
    pub timer_end: i64,
    /// Last timer length (seconds), the editor's starting value.
    pub timer_secs: u32,
    /// Snoozed alarm rings again at this unix time (0 = none).
    pub snooze_until: i64,
    /// Unix time of the alarm instance that last rang (never ring it twice).
    pub last_fired: i64,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { on: false, hour: 7, minute: 0, repeat: Repeat::Daily, snooze_min: 9, timer_end: 0, timer_secs: 300, snooze_until: 0, last_fired: 0 }
    }
}

/// Why the launcher is ringing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ring {
    /// The alarm (or a snoozed alarm) for this unix time.
    Alarm(i64),
    /// The countdown timer finished.
    Timer,
}

impl Settings {
    pub fn parse(s: &str) -> Self {
        let mut a = Settings::default();
        for line in s.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let v = v.trim();
            match k.trim() {
                "alarm_on" => a.on = v == "1" || v == "true",
                "alarm_time" => {
                    if let Some((h, m)) = v.split_once(':') {
                        if let (Ok(h), Ok(m)) = (h.trim().parse::<u8>(), m.trim().parse::<u8>()) {
                            if h < 24 && m < 60 {
                                a.hour = h;
                                a.minute = m;
                            }
                        }
                    }
                }
                "alarm_repeat" => a.repeat = Repeat::parse(v),
                "snooze_min" => a.snooze_min = v.parse().ok().filter(|m| (1..=60).contains(m)).unwrap_or(9),
                "timer_end" => a.timer_end = v.parse().unwrap_or(0),
                "timer_secs" => a.timer_secs = v.parse().ok().filter(|s| (1..=86_400).contains(s)).unwrap_or(300),
                "snooze_until" => a.snooze_until = v.parse().unwrap_or(0),
                "last_fired" => a.last_fired = v.parse().unwrap_or(0),
                _ => {}
            }
        }
        a
    }

    pub fn to_text(&self) -> String {
        format!(
            "# ZPOD Clock alarm + timer (written by the Clock app and the launcher)\n\
             alarm_on={}\nalarm_time={:02}:{:02}\nalarm_repeat={}\nsnooze_min={}\n\
             timer_end={}\ntimer_secs={}\nsnooze_until={}\nlast_fired={}\n",
            self.on as u8,
            self.hour,
            self.minute,
            self.repeat.name(),
            self.snooze_min,
            self.timer_end,
            self.timer_secs,
            self.snooze_until,
            self.last_fired
        )
    }

    pub fn time_text(&self) -> String {
        format!("{:02}:{:02}", self.hour, self.minute)
    }

    /// Unix time (local wall clock as seconds) of the alarm instance on the
    /// day of `now`, i.e. today at hh:mm.
    fn today_at(&self, now: &DateTime) -> i64 {
        DateTime { hour: self.hour, minute: self.minute, second: 0, ..*now }.to_unix()
    }

    /// What should ring at local time `now` (wall-clock fields) and unix
    /// time `now_unix`, if anything. Timer first, then snooze, then alarm.
    pub fn due(&self, now: &DateTime, now_unix: i64) -> Option<Ring> {
        if self.timer_end > 0 && now_unix >= self.timer_end {
            return Some(Ring::Timer);
        }
        if self.snooze_until > 0 && now_unix >= self.snooze_until {
            return Some(Ring::Alarm(self.snooze_until));
        }
        if !self.on || !self.repeat.on_day(now.weekday) {
            return None;
        }
        // compare in local wall-clock seconds (DateTime::to_unix of local fields)
        let local = now.to_unix();
        let at = self.today_at(now);
        if local >= at && local < at + LATE_GRACE_S && self.last_fired != at {
            return Some(Ring::Alarm(at));
        }
        None
    }

    /// After snoozing at `now_unix`.
    pub fn snooze(&mut self, ring: Ring, now_unix: i64) {
        if let Ring::Alarm(at) = ring {
            if at != self.snooze_until {
                self.last_fired = at;
            }
        }
        self.snooze_until = now_unix + self.snooze_min as i64 * 60;
        if ring == Ring::Timer {
            self.timer_end = 0;
        }
    }

    /// After dismissing (or the ring timing out).
    pub fn dismiss(&mut self, ring: Ring) {
        match ring {
            Ring::Timer => self.timer_end = 0,
            Ring::Alarm(at) => {
                if at != self.snooze_until {
                    self.last_fired = at;
                }
                self.snooze_until = 0;
                if self.repeat == Repeat::Once {
                    self.on = false;
                }
            }
        }
    }

    /// Seconds left on the timer (None if it is not running).
    pub fn timer_left(&self, now_unix: i64) -> Option<i64> {
        (self.timer_end > 0).then(|| (self.timer_end - now_unix).max(0))
    }

    /// Next ring as (days from today, hh:mm), for "Next: Tue 07:30".
    pub fn next_in_days(&self, now: &DateTime) -> Option<u8> {
        if !self.on {
            return None;
        }
        let past = (now.hour, now.minute) >= (self.hour, self.minute);
        (0..8u8).find(|&d| (d > 0 || !past) && self.repeat.on_day((now.weekday + d) % 7))
    }

    /// "today 07:30", "tomorrow 07:30", "Tue 07:30", or "off".
    pub fn next_text(&self, now: &DateTime) -> String {
        match self.next_in_days(now) {
            None => "off".into(),
            Some(0) => format!("today {}", self.time_text()),
            Some(1) => format!("tomorrow {}", self.time_text()),
            Some(d) => {
                let wd = (now.weekday + d) % 7;
                format!("{} {}", ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"][wd as usize], self.time_text())
            }
        }
    }
}

/// `<data dir>/clock/alarm.conf` without creating anything (the launcher is
/// root and must not create the app's directory with root ownership).
pub fn path() -> PathBuf {
    let base = std::env::var_os("ZPOD_DATA_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share/zpod")))
        .unwrap_or_else(|| "/var/lib/zpod".into());
    base.join("clock").join(FILE)
}

pub fn load() -> Settings {
    std::fs::read_to_string(path()).map(|s| Settings::parse(&s)).unwrap_or_default()
}

/// Save atomically. When root writes (launcher), the file is handed to the
/// owner of the directory so the Clock app can keep editing it.
pub fn save(a: &Settings) -> std::io::Result<()> {
    use std::os::unix::fs::MetadataExt;
    let p = path();
    if let Some(d) = p.parent() {
        if !d.exists() {
            std::fs::create_dir_all(d)?;
        }
    }
    let tmp = p.with_extension("tmp");
    std::fs::write(&tmp, a.to_text())?;
    if let Some(meta) = p.parent().and_then(|d| std::fs::metadata(d).ok()) {
        if unsafe { libc::geteuid() } == 0 {
            if let Ok(c) = std::ffi::CString::new(tmp.as_os_str().as_encoded_bytes()) {
                unsafe { libc::chown(c.as_ptr(), meta.uid(), meta.gid()) };
            }
        }
    }
    std::fs::rename(&tmp, &p)
}

/// 16-bit stereo 44.1 kHz WAV: four 880 Hz beeps then a pause, repeated to
/// about `secs` seconds. Played in a loop with `aplay` while ringing.
pub fn beep_wav(secs: u32) -> Vec<u8> {
    const RATE: u32 = 44_100;
    let n = RATE * secs.max(1);
    let mut pcm: Vec<i16> = Vec::with_capacity(n as usize * 2);
    // pattern period 1.6 s: (beep 0.12 s, gap 0.08 s) x 4, then 0.8 s quiet
    let period = RATE * 16 / 10;
    for i in 0..n {
        let t = i % period;
        let slot = t / (RATE / 5); // 0.2 s slots
        let in_slot = t % (RATE / 5);
        let on = slot < 4 && in_slot < RATE * 12 / 100;
        let v = if on {
            // 880 Hz square-ish sine via a cheap triangle -> softer than a square
            let ph = (i as u64 * 880 * 4 / RATE as u64) % 4; // quarter periods
            let frac = ((i as u64 * 880 * 4) % RATE as u64) as f32 / RATE as f32;
            let tri = match ph {
                0 => frac,
                1 => 1.0 - frac,
                2 => -frac,
                _ => frac - 1.0,
            };
            // 5 ms fade in/out against clicks
            let fade = (in_slot.min(RATE * 12 / 100 - in_slot) as f32 / (RATE as f32 * 0.005)).min(1.0);
            (tri * fade * 12_000.0) as i16
        } else {
            0
        };
        pcm.push(v);
        pcm.push(v);
    }
    let data_len = (pcm.len() * 2) as u32;
    let mut w = Vec::with_capacity(44 + data_len as usize);
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + data_len).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes()); // PCM
    w.extend_from_slice(&2u16.to_le_bytes()); // stereo
    w.extend_from_slice(&RATE.to_le_bytes());
    w.extend_from_slice(&(RATE * 4).to_le_bytes());
    w.extend_from_slice(&4u16.to_le_bytes());
    w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&data_len.to_le_bytes());
    for s in pcm {
        w.extend_from_slice(&s.to_le_bytes());
    }
    w
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> DateTime {
        DateTime::parse(s).unwrap()
    }

    #[test]
    fn roundtrip_and_defaults() {
        let a = Settings { on: true, hour: 6, minute: 45, repeat: Repeat::Weekdays, snooze_min: 10, timer_end: 123, timer_secs: 90, snooze_until: 0, last_fired: 7 };
        assert_eq!(Settings::parse(&a.to_text()), a);
        assert_eq!(Settings::parse("garbage\nalarm_time=25:00\nsnooze_min=0"), Settings::default());
    }

    #[test]
    fn rings_at_the_minute_once_and_respects_days() {
        let a = Settings { on: true, hour: 7, minute: 30, ..Default::default() };
        let t = at("2026-10-29 07:30:00"); // Thursday
        let u = t.to_unix();
        assert_eq!(a.due(&at("2026-10-29 07:29:59"), u - 1), None);
        assert_eq!(a.due(&t, u), Some(Ring::Alarm(u)));
        assert_eq!(a.due(&at("2026-10-29 07:31:30"), u + 90), Some(Ring::Alarm(u))); // late but in grace
        assert_eq!(a.due(&at("2026-10-29 07:33:00"), u + 180), None);
        let mut b = a.clone();
        b.dismiss(Ring::Alarm(u));
        assert_eq!(b.due(&at("2026-10-29 07:30:20"), u + 20), None); // not twice
        assert!(b.on);
        let we = Settings { repeat: Repeat::Weekends, ..a.clone() };
        assert_eq!(we.due(&t, u), None);
        let sat = at("2026-10-31 07:30:00");
        assert_eq!(we.due(&sat, sat.to_unix()), Some(Ring::Alarm(sat.to_unix())));
        let off = Settings { on: false, ..a };
        assert_eq!(off.due(&t, u), None);
    }

    #[test]
    fn snooze_then_dismiss_and_once_turns_off() {
        let mut a = Settings { on: true, hour: 7, minute: 30, repeat: Repeat::Once, snooze_min: 9, ..Default::default() };
        let t = at("2026-10-29 07:30:00");
        let u = t.to_unix();
        let r = a.due(&t, u).unwrap();
        a.snooze(r, u + 5);
        assert_eq!(a.snooze_until, u + 5 + 540);
        assert_eq!(a.due(&at("2026-10-29 07:31:00"), u + 60), None);
        let later = u + 5 + 540;
        let r2 = a.due(&DateTime::from_unix(later), later).unwrap();
        assert_eq!(r2, Ring::Alarm(later));
        a.dismiss(r2);
        assert_eq!(a.snooze_until, 0);
        assert!(!a.on); // "once"
        assert_eq!(a.last_fired, u);
    }

    #[test]
    fn timer_wins_and_clears() {
        let now = at("2026-10-29 12:00:00");
        let u = now.to_unix();
        let mut a = Settings { timer_end: u + 60, ..Default::default() };
        assert_eq!(a.timer_left(u), Some(60));
        assert_eq!(a.due(&now, u), None);
        assert_eq!(a.due(&now, u + 60), Some(Ring::Timer));
        a.dismiss(Ring::Timer);
        assert_eq!(a.timer_end, 0);
        assert_eq!(a.timer_left(u), None);
    }

    #[test]
    fn next_text() {
        let a = Settings { on: true, hour: 7, minute: 30, repeat: Repeat::Weekdays, ..Default::default() };
        assert_eq!(a.next_text(&at("2026-10-29 06:00:00")), "today 07:30"); // Thu
        assert_eq!(a.next_text(&at("2026-10-29 08:00:00")), "tomorrow 07:30"); // Fri
        assert_eq!(a.next_text(&at("2026-10-30 08:00:00")), "Mon 07:30"); // Fri -> Mon
        assert_eq!(Settings::default().next_text(&at("2026-10-30 08:00:00")), "off");
        assert_eq!(Repeat::Once.next(), Repeat::Daily);
    }

    #[test]
    fn wav_header() {
        let w = beep_wav(2);
        assert_eq!(&w[0..4], b"RIFF");
        assert_eq!(&w[8..16], b"WAVEfmt ");
        assert_eq!(w.len(), 44 + 44_100 * 2 * 4);
        assert!(w[44..].chunks(2).any(|c| i16::from_le_bytes([c[0], c[1]]).abs() > 8000));
    }
}
