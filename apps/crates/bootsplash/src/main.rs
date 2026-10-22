//! zpod-bootsplash: first pixels on the LCD, as early in boot as userspace
//! allows, then status lines + progress until the launcher takes over.
//!
//! * Waits (polling, no udev dependency) for /dev/spidev0.0, cold-inits the
//!   panel and draws the splash. Logs CLOCK_BOOTTIME of the first frame to
//!   stderr and /run/zpod/splash-first-frame (seconds since kernel start).
//! * Status lines come from (a) `journalctl -b -f -o cat _PID=1` when
//!   systemd/journald are present ("Starting ...", "Started ..."), and (b) a
//!   FIFO, /run/zpod/splash.fifo, that anything can write to (BusyBox init
//!   scripts: `zpod-bootsplash msg "Mounting data"`, `zpod-bootsplash progress 40`).
//! * Progress = max(lines seen / lines last boot, time / last boot duration),
//!   calibrated from /var/lib/zpod/bootsplash.cal, capped at 97% until handoff.
//! * SIGTERM (systemd Conflicts= from zpod-launcher, or the launcher itself
//!   via /run/zpod/bootsplash.pid): save calibration, exit leaving the frame
//!   on the panel and the init marker set, so the launcher warm-inits with no
//!   flash.

use std::io::{BufRead, BufReader, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use embedded_graphics::{
    pixelcolor::Rgb565,
    prelude::*,
    primitives::{PrimitiveStyle, RoundedRectangle},
};
use zpod_hal::{lcd::Lcd, sim::Sim};
use zpod_ui::{
    fb::Framebuffer,
    fill, rect,
    text::{self, Align},
    theme::*,
    Font,
};

const FIFO: &str = "/run/zpod/splash.fifo";
const PIDFILE: &str = "/run/zpod/bootsplash.pid";
const CAL: &str = "/var/lib/zpod/bootsplash.cal";
const FIRST_FRAME: &str = "/run/zpod/splash-first-frame";
const MAX_RUN: Duration = Duration::from_secs(180);
const LINES: usize = 7;
const LINE_Y0: i32 = 128;
const LINE_H: i32 = 14;
const BAR: (i32, i32, u32, u32) = (40, 104, 240, 8);

static TERM: AtomicBool = AtomicBool::new(false);
extern "C" fn on_term(_: libc::c_int) {
    TERM.store(true, Ordering::SeqCst);
}

enum Msg {
    Line(String),
    Progress(f32),
    Done,
}

fn boottime() -> f64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &mut ts) };
    ts.tv_sec as f64 + ts.tv_nsec as f64 / 1e9
}

/// Clean a systemd status line for a 300 px wide tiny font.
pub fn tidy(line: &str) -> Option<String> {
    let l = line.trim();
    if l.is_empty() {
        return None;
    }
    // "Starting zpod-buttons.service - ZPOD buttons..." -> "Starting ZPOD buttons..."
    let l = match l.split_once(" - ") {
        Some((head, desc)) if head.contains('.') => {
            let verb = head.split_whitespace().next().unwrap_or("");
            format!("{verb} {desc}")
        }
        _ => l.to_string(),
    };
    Some(l.chars().filter(|c| !c.is_control()).collect())
}

struct Cal {
    lines: f32,
    secs: f32,
}

fn load_cal() -> Cal {
    let s = std::fs::read_to_string(CAL).unwrap_or_default();
    let mut it = s.split_whitespace().filter_map(|x| x.parse::<f32>().ok());
    Cal { lines: it.next().unwrap_or(80.0).max(10.0), secs: it.next().unwrap_or(30.0).max(5.0) }
}

fn save_cal(lines: usize, secs: f32) {
    if lines < 5 {
        return;
    }
    let _ = std::fs::create_dir_all("/var/lib/zpod");
    let tmp = format!("{CAL}.tmp");
    if std::fs::write(&tmp, format!("{lines} {secs:.1}\n")).is_ok() {
        let _ = std::fs::rename(&tmp, CAL);
    }
}

fn spawn_journal(tx: mpsc::Sender<Msg>) {
    std::thread::spawn(move || {
        let start = Instant::now();
        // journald may not be up yet: retry for a while.
        while start.elapsed() < Duration::from_secs(60) && !TERM.load(Ordering::SeqCst) {
            if !std::path::Path::new("/run/systemd/journal/socket").exists() {
                if !std::path::Path::new("/run/systemd/system").exists() && start.elapsed() > Duration::from_secs(3) {
                    return; // not systemd: FIFO only
                }
                std::thread::sleep(Duration::from_millis(200));
                continue;
            }
            let child = std::process::Command::new("journalctl")
                .args(["-b", "-f", "-o", "cat", "--no-pager", "-n", "all", "_PID=1"])
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                .spawn();
            let Ok(mut child) = child else { return };
            let out = child.stdout.take().unwrap();
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                if let Some(l) = tidy(&line) {
                    if tx.send(Msg::Line(l)).is_err() {
                        let _ = child.kill();
                        return;
                    }
                }
            }
            let _ = child.wait();
            std::thread::sleep(Duration::from_millis(500));
        }
    });
}

fn spawn_fifo(tx: mpsc::Sender<Msg>) {
    let _ = std::fs::create_dir_all("/run/zpod");
    let _ = std::fs::remove_file(FIFO);
    let c = std::ffi::CString::new(FIFO).unwrap();
    if unsafe { libc::mkfifo(c.as_ptr(), 0o620) } != 0 {
        return;
    }
    std::thread::spawn(move || loop {
        // open blocks until a writer appears; reopen after each writer closes
        let Ok(f) = std::fs::File::open(FIFO) else { return };
        for line in BufReader::new(f).lines().map_while(Result::ok) {
            let msg = if let Some(p) = line.strip_prefix("progress ") {
                p.trim().trim_end_matches('%').parse::<f32>().ok().map(|v| Msg::Progress(v / 100.0))
            } else if line.trim() == "done" {
                Some(Msg::Done)
            } else {
                tidy(&line).map(Msg::Line)
            };
            if let Some(m) = msg {
                if tx.send(m).is_err() {
                    return;
                }
            }
        }
    });
}

/// Sim/demo: feed lines from a file at a steady rate.
fn spawn_feed(tx: mpsc::Sender<Msg>, path: String) {
    std::thread::spawn(move || {
        for line in std::fs::read_to_string(path).unwrap_or_default().lines() {
            std::thread::sleep(Duration::from_millis(60));
            if let Some(l) = tidy(line) {
                if tx.send(Msg::Line(l)).is_err() {
                    return;
                }
            }
        }
    });
}

// ------------------------------------------------------------------ drawing

fn draw_static(fb: &mut Framebuffer) {
    fill(fb, 0, 0, 320, 240, BG);
    // logo: four rising bars + wordmark
    let colors = [rgb(255, 90, 120), ACCENT, rgb(0, 216, 120), CYAN];
    for (i, c) in colors.iter().enumerate() {
        let h = 22 + i as u32 * 6;
        let _ = RoundedRectangle::with_equal_corners(rect(70 + i as i32 * 12, 70 - h as i32, 8, h), Size::new(3, 3))
            .into_styled(PrimitiveStyle::with_fill(*c))
            .draw(fb);
    }
    text::draw(fb, Font::Huge, "ZPOD", 128, 34, FG);
    text::draw_aligned(fb, Font::Small, "starting up", 160, 82, Align::Center, DIM);
}

fn draw_progress(fb: &mut Framebuffer, frac: f32) {
    let (x, y, w, h) = BAR;
    let _ = RoundedRectangle::with_equal_corners(rect(x, y, w, h), Size::new(4, 4))
        .into_styled(PrimitiveStyle::with_fill(PANEL))
        .draw(fb);
    let fw = ((w as f32) * frac.clamp(0.0, 1.0)) as u32;
    if fw >= 4 {
        let _ = RoundedRectangle::with_equal_corners(rect(x, y, fw, h), Size::new(4, 4))
            .into_styled(PrimitiveStyle::with_fill(ACCENT))
            .draw(fb);
    }
}

fn draw_lines(fb: &mut Framebuffer, lines: &std::collections::VecDeque<String>) {
    fill(fb, 0, LINE_Y0, 320, (LINES as i32 * LINE_H) as u32, BG);
    let n = lines.len();
    for (i, l) in lines.iter().enumerate() {
        let age = n - 1 - i; // 0 = newest
        let c: Rgb565 = match age {
            0 => FG,
            1 | 2 => DIM,
            _ => FAINT,
        };
        let y = LINE_Y0 + (LINES - n + i) as i32 * LINE_H;
        text::draw_fit(fb, Font::Tiny, l, 12, y, 296, c);
    }
}

// --------------------------------------------------------------------- main

fn client(args: &[String]) -> i32 {
    // `zpod-bootsplash msg <text...>` / `progress <0-100>` / `done`
    let line = match args[0].as_str() {
        "msg" => args[1..].join(" "),
        "progress" => format!("progress {}", args.get(1).map(String::as_str).unwrap_or("0")),
        "done" => "done".into(),
        _ => return 2,
    };
    // O_NONBLOCK: never hang a boot script if the splash is not running.
    use std::os::unix::fs::OpenOptionsExt;
    if let Ok(mut f) = std::fs::OpenOptions::new().write(true).custom_flags(libc::O_NONBLOCK).open(FIFO) {
        let _ = writeln!(f, "{line}");
    }
    0
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !args.is_empty() {
        if args[0] == "-h" || args[0] == "--help" {
            println!("usage: zpod-bootsplash            run the splash (as root, early in boot)\n       zpod-bootsplash msg <text>   add a status line\n       zpod-bootsplash progress <n> set progress (0-100)\n       zpod-bootsplash done         finish (full bar), keep frame, exit");
            return;
        }
        std::process::exit(client(&args));
    }
    let t0 = Instant::now();
    unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = on_term as *const () as usize;
        libc::sigaction(libc::SIGTERM, &sa, std::ptr::null_mut());
        libc::sigaction(libc::SIGINT, &sa, std::ptr::null_mut());
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }
    let mut sim = Sim::from_env();
    if sim.is_none() {
        let _ = std::fs::create_dir_all("/run/zpod");
        let _ = std::fs::write(PIDFILE, format!("{}\n", std::process::id()));
    }

    // Wait for the SPI controller + spidev (module load via udev coldplug).
    let mut lcd = None;
    if sim.is_none() {
        let deadline = Instant::now() + Duration::from_secs(30);
        while lcd.is_none() && !TERM.load(Ordering::SeqCst) {
            if std::path::Path::new(zpod_hal::lcd::SPI_DEV).exists() {
                match Lcd::open(Some(true)) {
                    Ok(l) => lcd = Some(l),
                    Err(e) if Instant::now() > deadline => {
                        eprintln!("zpod-bootsplash: display: {e}");
                        return;
                    }
                    Err(_) => {}
                }
            } else if Instant::now() > deadline {
                eprintln!("zpod-bootsplash: {} did not appear", zpod_hal::lcd::SPI_DEV);
                return;
            }
            if lcd.is_none() {
                std::thread::sleep(Duration::from_millis(25));
            }
        }
    }
    let present = |fb: &mut Framebuffer, lcd: &mut Option<Lcd>, sim: &mut Option<Sim>| {
        if let Some(l) = lcd.as_mut() {
            l.flush(fb);
        } else if let Some(s) = sim.as_mut() {
            s.present(fb);
        }
    };

    let mut fb = Framebuffer::default();
    let cal = load_cal();
    draw_static(&mut fb);
    draw_progress(&mut fb, 0.0);
    present(&mut fb, &mut lcd, &mut sim);
    let first = boottime();
    eprintln!(
        "zpod-bootsplash: first frame at {first:.2}s after kernel start ({} ms after exec)",
        t0.elapsed().as_millis()
    );
    if sim.is_none() {
        let _ = std::fs::write(FIRST_FRAME, format!("{first:.3}\n"));
    }

    let (tx, rx) = mpsc::channel();
    if let Ok(feed) = std::env::var("ZPOD_SPLASH_FEED") {
        spawn_feed(tx.clone(), feed);
    } else if sim.is_none() {
        spawn_journal(tx.clone());
        spawn_fifo(tx.clone());
    }
    drop(tx);

    let mut lines = std::collections::VecDeque::new();
    let mut seen = 0usize;
    let mut forced: Option<f32> = None;
    let mut shown = -1.0f32;
    let mut done = false;
    // Sim: pretend we started 6 s into boot so the time-based bar is visible.
    let boot_at_start = if sim.is_some() { 6.0 } else { first as f32 };
    loop {
        if TERM.load(Ordering::SeqCst) || t0.elapsed() > MAX_RUN {
            break;
        }
        let mut new_lines = false;
        while let Ok(m) = rx.try_recv() {
            match m {
                Msg::Line(l) => {
                    seen += 1;
                    lines.push_back(l);
                    if lines.len() > LINES {
                        lines.pop_front();
                    }
                    new_lines = true;
                }
                Msg::Progress(p) => forced = Some(p),
                Msg::Done => done = true,
            }
        }
        if new_lines {
            draw_lines(&mut fb, &lines);
        }
        let by_lines = seen as f32 / cal.lines;
        let by_time = (boot_at_start + t0.elapsed().as_secs_f32()) / cal.secs;
        let mut p = forced.unwrap_or(by_lines.max(by_time)).min(0.97);
        if done {
            p = 1.0;
        }
        if (p - shown).abs() >= 0.004 {
            draw_progress(&mut fb, p);
            shown = p;
        }
        if fb.is_dirty() {
            present(&mut fb, &mut lcd, &mut sim);
        }
        if done {
            break;
        }
        // ~12 fps max; the sim advances its script clock here.
        match sim.as_mut() {
            Some(s) => {
                let _ = s.next_event(Some(Duration::from_millis(80)));
            }
            None => std::thread::sleep(Duration::from_millis(80)),
        }
    }
    if sim.is_none() {
        save_cal(seen, boottime() as f32);
        let _ = std::fs::remove_file(PIDFILE);
        let _ = std::fs::remove_file(FIFO);
    }
    // Leave the last frame and the init marker: the launcher warm-inits.
    drop(lcd);
}

#[cfg(test)]
mod tests {
    use super::tidy;
    #[test]
    fn tidies_systemd_lines() {
        assert_eq!(tidy("Starting zpod-buttons.service - ZPOD buttons...").unwrap(), "Starting ZPOD buttons...");
        assert_eq!(tidy("Reached target sysinit.target - System Initialization.").unwrap(), "Reached System Initialization.");
        assert_eq!(tidy("Mounting data - with dash").unwrap(), "Mounting data - with dash");
        assert!(tidy("   ").is_none());
    }
}
