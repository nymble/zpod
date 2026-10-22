//! Headless simulator: scripted key input + PNG screenshots.
//!
//! `ZPOD_SIM=<out dir>` selects it; `ZPOD_SIM_SCRIPT=<file>` (or `-` for
//! stdin) supplies commands, one per line:
//!
//! ```text
//! key <name>          press + release (up down left right ok home play
//!                     triangle square circle x volup voldown)
//! hold <name> <ms>    press, autorepeat every 120 ms, release
//! wait <ms>           let the app run (timers, animations)
//! shot <name>         save the frame currently on the "panel" as <name>.png
//! exit                end (the process exits with status 0)
//! # comment
//! ```
//! When the script ends the process exits.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use zpod_core::{Event, Key, KeyState};
use zpod_ui::fb::Framebuffer;

pub struct Sim {
    pub out: PathBuf,
    lines: VecDeque<String>,
    queue: VecDeque<(Option<Instant>, Event)>,
    wait_until: Option<Instant>,
    /// Copy of what the app last flushed (what the panel would show).
    pub shown: Framebuffer,
    pub frames: u64,
}

impl Sim {
    pub fn from_env() -> Option<Self> {
        let out = PathBuf::from(std::env::var_os("ZPOD_SIM")?);
        let _ = std::fs::create_dir_all(&out);
        let script = std::env::var("ZPOD_SIM_SCRIPT").ok();
        let lines: VecDeque<String> = match script.as_deref() {
            Some("-") => BufReader::new(std::io::stdin()).lines().map_while(Result::ok).collect(),
            Some(p) => std::fs::read_to_string(p).unwrap_or_default().lines().map(|s| s.to_string()).collect(),
            None => VecDeque::new(),
        };
        Some(Sim::new(out, lines))
    }

    pub fn new(out: PathBuf, lines: VecDeque<String>) -> Self {
        Sim { out, lines, queue: VecDeque::new(), wait_until: None, shown: Framebuffer::default(), frames: 0 }
    }

    pub fn present(&mut self, fb: &mut Framebuffer) {
        for r in fb.take_dirty() {
            for y in r.y0 as usize..=r.y1 as usize {
                let a = y * fb.width + r.x0 as usize;
                let b = y * fb.width + r.x1 as usize;
                self.shown.px[a..=b].copy_from_slice(&fb.px[a..=b]);
            }
        }
        self.frames += 1;
    }

    pub fn save_png(&self, name: &str) -> std::io::Result<PathBuf> {
        let path = self.out.join(format!("{name}.png"));
        write_png(&path, &self.shown)?;
        eprintln!("sim: saved {}", path.display());
        Ok(path)
    }

    /// Next scripted event, honouring waits; None on timeout.
    pub fn next_event(&mut self, timeout: Option<Duration>) -> Option<Event> {
        let deadline = timeout.map(|t| Instant::now() + t);
        loop {
            let now = Instant::now();
            if let Some(&(at, ev)) = self.queue.front() {
                match at {
                    Some(t) if t > now => {
                        let until = deadline.map_or(t, |d| d.min(t));
                        std::thread::sleep(until.saturating_duration_since(now));
                        if deadline.is_some_and(|d| Instant::now() >= d) && Instant::now() < t {
                            return None;
                        }
                        continue;
                    }
                    _ => {
                        self.queue.pop_front();
                        return Some(ev);
                    }
                }
            }
            if let Some(w) = self.wait_until {
                if w > now {
                    let until = deadline.map_or(w, |d| d.min(w));
                    std::thread::sleep(until.saturating_duration_since(now));
                    if Instant::now() < w {
                        return None;
                    }
                }
                self.wait_until = None;
            }
            let Some(line) = self.lines.pop_front() else {
                eprintln!("sim: script finished ({} frames)", self.frames);
                std::process::exit(0);
            };
            let mut it = line.split_whitespace();
            match (it.next(), it.next(), it.next()) {
                (None, _, _) => {}
                (Some(c), _, _) if c.starts_with('#') => {}
                (Some("key"), Some(k), _) => {
                    if let Some(key) = Key::from_name(k) {
                        self.queue.push_back((None, Event { key, state: KeyState::Press }));
                        self.queue.push_back((None, Event { key, state: KeyState::Release }));
                    }
                }
                (Some("hold"), Some(k), ms) => {
                    if let Some(key) = Key::from_name(k) {
                        let ms: u64 = ms.and_then(|m| m.parse().ok()).unwrap_or(500);
                        let t0 = Instant::now();
                        self.queue.push_back((None, Event { key, state: KeyState::Press }));
                        let mut t = 400;
                        while t < ms {
                            self.queue.push_back((Some(t0 + Duration::from_millis(t)), Event { key, state: KeyState::Repeat }));
                            t += 120;
                        }
                        self.queue.push_back((Some(t0 + Duration::from_millis(ms)), Event { key, state: KeyState::Release }));
                    }
                }
                (Some("wait"), Some(ms), _) => {
                    let ms: u64 = ms.parse().unwrap_or(0);
                    self.wait_until = Some(Instant::now() + Duration::from_millis(ms));
                }
                (Some("shot"), Some(name), _) => {
                    let _ = self.save_png(name);
                }
                (Some("exit"), _, _) => {
                    eprintln!("sim: exit");
                    std::process::exit(0);
                }
                _ => eprintln!("sim: bad line: {line}"),
            }
        }
    }
}

pub fn write_png(path: &std::path::Path, fb: &Framebuffer) -> std::io::Result<()> {
    let f = std::fs::File::create(path)?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(f), fb.width as u32, fb.height as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header().map_err(std::io::Error::other)?;
    w.write_image_data(&fb.to_rgb888()).map_err(std::io::Error::other)?;
    Ok(())
}
