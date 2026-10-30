//! zpod-battery (M6): battery logger and low-battery guard in one small
//! daemon (replaces the Python zpod_battery_guard.py at boot: less CPU on
//! the single-core Zero while it boots).
//!
//! * Every 30 s: read the MAX17048 (I2C-1, 0x36): SOC, VCELL, CRATE.
//! * Every 60 s: append one line to /data/zpod/battery.log (format in
//!   zpod_hal::battery), rotated at 512 KiB to battery.log.1. A finished
//!   discharge (unplugged -> next charge) is added to battery-sessions.log.
//!   Nothing is written while the clock is unset (before 2025) or /data is
//!   missing.
//! * Guard (same rule as M3-M5): power off when, on 3 consecutive reads
//!   30 s apart, SOC < [battery] shutdown_pct (5) AND VCELL < shutdown_mv
//!   (3600) AND CRATE <= 0. shutdown_pct = 0 disables the guard.
//! * No gauge: logs that once, keeps retrying every 60 s, never exits.
//!
//! `zpod-battery --once` prints one reading and exits (diagnostics).

use std::io::Write;
use std::time::Duration;

use zpod_drivers::max17048::Max17048;
use zpod_hal::battery::{self, Sample, Session};
use zpod_hal::conf;

const INTERVAL: Duration = Duration::from_secs(30);
const NEEDED: u32 = 3;
const CLOCK_OK: i64 = 1_735_689_600; // 2025-01-01

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn limits() -> (f32, f32) {
    let t = std::fs::read_to_string(conf::path()).unwrap_or_default();
    let pct = conf::get_in(&t, "battery", "shutdown_pct").and_then(|v| v.parse().ok()).unwrap_or(5.0);
    let mv = conf::get_in(&t, "battery", "shutdown_mv").and_then(|v| v.parse().ok()).unwrap_or(3600.0);
    (pct, mv)
}

pub fn low(s: &Sample, pct: f32, mv: f32) -> bool {
    pct > 0.0 && s.soc < pct && s.volts * 1000.0 < mv && s.rate <= 0.0
}

type Gauge = Max17048<linux_embedded_hal::I2cdev>;

fn open() -> Option<Gauge> {
    linux_embedded_hal::I2cdev::new(std::env::var("ZPOD_I2C").unwrap_or_else(|_| "/dev/i2c-1".into())).ok().map(Max17048::new)
}

fn read(g: &mut Gauge) -> Option<Sample> {
    let soc = g.soc_percent().ok()?;
    let volts = g.vcell().ok()?;
    let rate = g.crate_pct_per_hour().ok()?;
    Some(Sample { t: now(), soc, volts, rate, charging: rate > battery::CHARGING_RATE })
}

fn append(path: &std::path::Path, line: &str) -> std::io::Result<()> {
    std::fs::OpenOptions::new().create(true).append(true).open(path)?.write_all(line.as_bytes())
}

fn rotate_if_big() {
    let p = battery::log_path();
    if std::fs::metadata(&p).map(|m| m.len() > battery::LOG_CAP).unwrap_or(false) {
        let _ = std::fs::rename(&p, p.with_extension("log.1"));
    }
}

fn record_session(s: &Session) {
    let p = battery::sessions_path();
    let mut lines: Vec<String> = std::fs::read_to_string(&p).unwrap_or_default().lines().map(str::to_string).collect();
    lines.push(battery::format_session(s).trim_end().to_string());
    let keep = lines.len().saturating_sub(battery::SESSIONS_KEEP);
    let body = lines[keep..].join("\n") + "\n";
    let tmp = p.with_extension("tmp");
    if std::fs::write(&tmp, body).and_then(|_| std::fs::rename(&tmp, &p)).is_err() {
        eprintln!("zpod-battery: cannot write {}", p.display());
    }
}

fn main() {
    let once = std::env::args().any(|a| a == "--once");
    let (pct, mv) = limits();
    if once {
        match open().as_mut().and_then(read) {
            Some(s) => print!("{}", battery::format_sample(&s)),
            None => {
                eprintln!("zpod-battery: no MAX17048 at 0x36 on /dev/i2c-1");
                std::process::exit(1);
            }
        }
        return;
    }
    eprintln!("zpod-battery: logging to {} every 60 s; guard {}", battery::log_path().display(),
        if pct > 0.0 { format!("powers off below {pct}% and {mv} mV while discharging") } else { "disabled (shutdown_pct = 0)".into() });
    // resume the discharge in progress from the log tail
    let (hist, _) = battery::load();
    let mut dis_start: Option<(i64, f32)> = battery::current_session(&hist).filter(|_| !hist.last().map(|s| s.charging).unwrap_or(true)).map(|s| (s.start, s.soc0));
    let mut prev: Option<Sample> = hist.last().copied();
    let mut gauge: Option<Gauge> = None;
    let mut missing_said = false;
    let mut hits = 0u32;
    let mut tick = 0u64;
    loop {
        if gauge.is_none() {
            gauge = open();
        }
        let s = gauge.as_mut().and_then(read);
        match s {
            None => {
                gauge = None;
                hits = 0;
                if !missing_said {
                    eprintln!("zpod-battery: fuel gauge not answering (MAX17048 0x36); retrying every 60 s");
                    missing_said = true;
                }
                std::thread::sleep(Duration::from_secs(60));
                continue;
            }
            Some(s) => {
                if missing_said {
                    eprintln!("zpod-battery: fuel gauge back: {:.1}% {:.3} V", s.soc, s.volts);
                    missing_said = false;
                }
                if low(&s, pct, mv) {
                    hits += 1;
                    eprintln!("zpod-battery: LOW {hits}/{NEEDED}: {:.1}% {:.0} mV {:+.1}%/h", s.soc, s.volts * 1000.0, s.rate);
                    if hits >= NEEDED {
                        eprintln!("zpod-battery: battery empty, powering off");
                        let _ = append(&battery::log_path(), &battery::format_sample(&s));
                        let _ = std::process::Command::new("systemctl").arg("poweroff").status();
                        return;
                    }
                } else {
                    hits = 0;
                }
                if tick % 2 == 0 && s.t >= CLOCK_OK && battery::dir().is_dir() {
                    if let Some(done) = battery::session_ended(dis_start, prev.as_ref(), &s) {
                        record_session(&done);
                    }
                    if s.charging {
                        dis_start = None;
                    } else if dis_start.is_none() {
                        dis_start = Some((s.t, s.soc));
                    }
                    rotate_if_big();
                    if let Err(e) = append(&battery::log_path(), &battery::format_sample(&s)) {
                        eprintln!("zpod-battery: {}: {e}", battery::log_path().display());
                    }
                    prev = Some(s);
                }
            }
        }
        tick += 1;
        std::thread::sleep(INTERVAL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn guard_rule_matches_m3() {
        let s = |soc, volts, rate| Sample { t: 0, soc, volts, rate, charging: false };
        assert!(low(&s(4.0, 3.5, -3.0), 5.0, 3600.0));
        assert!(!low(&s(4.0, 3.7, -3.0), 5.0, 3600.0), "voltage still fine");
        assert!(!low(&s(4.0, 3.5, 2.0), 5.0, 3600.0), "charging");
        assert!(!low(&s(6.0, 3.5, -3.0), 5.0, 3600.0));
        assert!(!low(&s(1.0, 3.3, -3.0), 0.0, 3600.0), "disabled");
    }
}
