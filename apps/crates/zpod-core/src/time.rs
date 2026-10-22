//! Calendar time without an OS (civil-from-days algorithm, H. Hinnant).

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct DateTime {
    pub year: i32,
    pub month: u8, // 1..=12
    pub day: u8,   // 1..=31
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub weekday: u8, // 0 = Sunday
}

impl DateTime {
    /// Seconds since the Unix epoch (+ offset already applied) -> DateTime.
    pub fn from_unix(secs: i64) -> Self {
        let days = secs.div_euclid(86_400);
        let sod = secs.rem_euclid(86_400);
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = (doy - (153 * mp + 2) / 5 + 1) as u8;
        let m = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
        let year = (if m <= 2 { y + 1 } else { y }) as i32;
        DateTime {
            year,
            month: m,
            day: d,
            hour: (sod / 3600) as u8,
            minute: (sod % 3600 / 60) as u8,
            second: (sod % 60) as u8,
            weekday: ((days + 4).rem_euclid(7)) as u8,
        }
    }

    /// Inverse of [`from_unix`](Self::from_unix) (weekday ignored).
    pub fn to_unix(&self) -> i64 {
        let y = self.year as i64 - if self.month <= 2 { 1 } else { 0 };
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let m = self.month as i64;
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + self.day as i64 - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        let days = era * 146_097 + doe - 719_468;
        days * 86_400 + self.hour as i64 * 3600 + self.minute as i64 * 60 + self.second as i64
    }

    /// Parse "YYYY-MM-DD HH:MM[:SS]" (or with 'T').
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        let (d, t) = s.split_once([' ', 'T'])?;
        let mut dp = d.split('-');
        let year = dp.next()?.parse().ok()?;
        let month = dp.next()?.parse().ok()?;
        let day = dp.next()?.parse().ok()?;
        let mut tp = t.split(':');
        let hour = tp.next()?.parse().ok()?;
        let minute = tp.next()?.parse().ok()?;
        let second = tp.next().map(|x| x.parse().ok()).unwrap_or(Some(0))?;
        let dt = DateTime { year, month, day, hour, minute, second, weekday: 0 };
        Some(DateTime::from_unix(dt.to_unix()))
    }

    pub fn weekday_name(&self) -> &'static str {
        ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"][self.weekday as usize % 7]
    }

    pub fn month_name(&self) -> &'static str {
        [
            "January", "February", "March", "April", "May", "June", "July", "August", "September", "October",
            "November", "December",
        ][(self.month as usize + 11) % 12]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn epoch_and_known_dates() {
        let e = DateTime::from_unix(0);
        assert_eq!((e.year, e.month, e.day, e.weekday), (1970, 1, 1, 4));
        // 2026-10-17 17:35:00 UTC is a Saturday
        let t = DateTime::from_unix(1_792_258_500);
        assert_eq!((t.year, t.month, t.day, t.hour, t.minute, t.weekday), (2026, 10, 17, 17, 35, 6));
        let leap = DateTime::from_unix(951_782_400); // 2000-02-29
        assert_eq!((leap.year, leap.month, leap.day), (2000, 2, 29));
        assert_eq!(leap.to_unix(), 951_782_400);
        assert_eq!(t.to_unix(), 1_792_258_500);
        let p = DateTime::parse("2026-10-17 17:35").unwrap();
        assert_eq!(p, t);
    }
}
