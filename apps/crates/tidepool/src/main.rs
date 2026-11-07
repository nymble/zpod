//! Tidepool for ZPOD: a calm, slowly drifting Euclidean ambient generator.
//!
//! Four polymetric Euclidean lanes (drone 32, pulse 16, bell 13, glass 9
//! steps) play a scale through the shared zpod-music-core synth with a soft
//! pad patch; melodies random-walk and rhythms mutate every few bars, all
//! from one seed, so a seed always sounds the same (generate.rs).
//!
//! Buttons: Play start/stop · Up/Dn tempo · Lt/Rt scale · Tri key (+5th)
//! Cir/OK next seed · Sq level (Soft/Medium/Full) · X help · Home exit.
//! Starts stopped; MPD is paused while open and left paused on exit.

mod ui;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use zpod_core::{Key, KeyState, StatusInfo};
use zpod_hal::Platform;
use zpod_music_core::generate::{step_samples, NoteEvent, Tidepool, SCALES};
use zpod_music_core::harmony::Root;
use zpod_music_core::{Adsr, AudioRuntime, Engine, SAMPLE_RATE};

static STOP: AtomicBool = AtomicBool::new(false);
extern "C" fn on_sigterm(_: libc::c_int) {
    STOP.store(true, Ordering::SeqCst);
}

pub const LEVELS: [(&str, f32); 3] = [("Soft", 0.35), ("Medium", 0.5), ("Full", 0.68)];
pub const TEMPO_MIN: u32 = 40;
pub const TEMPO_MAX: u32 = 120;

/// Settings (persisted) + live generator, shared by UI and audio thread.
pub struct Gen {
    pub seed: u32,
    pub scale: usize,
    pub root: u8,
    pub tempo: u32,
    pub level: usize,
    pub playing: bool,
    pub exit: bool,
    pub tp: Tidepool,
    acc: u32,
    pending: Vec<NoteEvent>,
    gain: f32,
}

impl Gen {
    fn new(seed: u32, scale: usize, root: u8, tempo: u32, level: usize) -> Self {
        let tp = Tidepool::new(seed, SCALES[scale % SCALES.len()], Root { pc: root % 12 });
        Self { seed, scale, root, tempo, level, playing: false, exit: false, tp, acc: 0, pending: Vec::new(), gain: 0.0 }
    }
    /// Rebuild the sequencer for new seed/scale/key (old notes end first).
    pub fn regen(&mut self) {
        self.tp.stop(&mut self.pending);
        self.tp = Tidepool::new(self.seed, SCALES[self.scale % SCALES.len()], Root { pc: self.root % 12 });
        self.acc = 0;
    }
    /// Advance `frames` of time; collect note events into `pending`.
    pub fn advance(&mut self, frames: u32) {
        if self.playing && !self.exit {
            self.acc += frames;
            let st = step_samples(SAMPLE_RATE, self.tempo);
            while self.acc >= st {
                self.acc -= st;
                self.tp.step(&mut self.pending);
            }
        } else if self.tp.held_count() > 0 {
            self.tp.stop(&mut self.pending);
        }
    }
    /// Audio-thread side: apply events and the smoothed master gain.
    fn drive(&mut self, eng: &mut Engine, frames: usize) -> bool {
        self.advance(frames as u32);
        for e in self.pending.drain(..) {
            match e {
                NoteEvent::On(m, v) => {
                    eng.note_on(m, v as f32 / 127.0);
                }
                NoteEvent::Off(m) => eng.note_off(m),
            }
        }
        let target = if self.exit { 0.0 } else { LEVELS[self.level % LEVELS.len()].1 };
        self.gain += (target - self.gain) * 0.08; // ~70 ms glide, no clicks
        eng.master = self.gain;
        !(self.exit && self.gain < 0.004)
    }
}

fn settings_path() -> std::path::PathBuf {
    zpod_hal::data_dir("tidepool").join("settings")
}

fn load() -> Gen {
    let (mut seed, mut scale, mut root, mut tempo, mut level) = (1u32, 0usize, 2u8, 72u32, 0usize);
    if let Ok(s) = std::fs::read_to_string(settings_path()) {
        for l in s.lines() {
            let Some((k, v)) = l.split_once('=') else { continue };
            let v = v.trim();
            match k.trim() {
                "seed" => seed = v.parse().unwrap_or(seed),
                "scale" => scale = v.parse::<usize>().unwrap_or(0) % SCALES.len(),
                "root" => root = v.parse::<u8>().unwrap_or(2) % 12,
                "tempo" => tempo = v.parse::<u32>().unwrap_or(72).clamp(TEMPO_MIN, TEMPO_MAX),
                "level" => level = v.parse::<usize>().unwrap_or(0) % LEVELS.len(),
                _ => {}
            }
        }
    }
    Gen::new(seed, scale, root, tempo, level)
}

fn save(g: &Gen) {
    let s = format!("seed={}\nscale={}\nroot={}\ntempo={}\nlevel={}\n", g.seed, g.scale, g.root, g.tempo, g.level);
    let p = settings_path();
    let tmp = p.with_extension("tmp");
    if std::fs::write(&tmp, s).is_ok() {
        let _ = std::fs::rename(tmp, p);
    }
}

fn main() {
    if std::env::args().nth(1).as_deref() == Some("--help") {
        eprintln!("tidepool: ambient generator; buttons only (see X help in the app)");
        return;
    }
    if std::env::args().nth(1).as_deref() == Some("--selftest") {
        let secs: u64 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(8).min(60);
        std::process::exit(selftest(secs));
    }
    let mut p = match Platform::open() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("tidepool: cannot open display: {e}");
            std::process::exit(1);
        }
    };
    unsafe {
        libc::signal(libc::SIGTERM, on_sigterm as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, on_sigterm as *const () as libc::sighandler_t);
    }
    let gen = Arc::new(Mutex::new(load()));
    let g2 = Arc::clone(&gen);
    let audio = if p.is_sim() {
        None
    } else {
        let mut patched = false;
        match AudioRuntime::start_with(move |eng, frames| {
            if !patched {
                patched = true;
                // first period: soft pad patch (slow attack, long tail)
                eng.set_patch(Adsr { a: 0.35, d: 0.9, s: 0.55, r: 2.4 }, 1500.0, 0.12);
            }
            g2.lock().map(|mut g| g.drive(eng, frames)).unwrap_or(false)
        }) {
            Ok(a) => Some(a),
            Err(e) => {
                eprintln!("tidepool: audio not available: {e}");
                None
            }
        }
    };
    let silent = audio.is_none();
    eprintln!("tidepool: started (audio {})", if silent { "off: preview only" } else { "on" });

    let mut help = false;
    let mut status: StatusInfo = p.status.poll();
    let mut last = Instant::now();
    let mut dirty = true;
    let mut last_draw = Instant::now();
    loop {
        if STOP.load(Ordering::Relaxed) {
            break;
        }
        if silent {
            // no DAC (simulator, or audio busy): keep the picture moving
            let el = last.elapsed();
            last = Instant::now();
            let mut g = gen.lock().unwrap();
            g.advance((el.as_secs_f32() * SAMPLE_RATE as f32) as u32);
            g.pending.clear();
        }
        let st = p.status.poll();
        if st != status {
            status = st;
            dirty = true;
        }
        if last_draw.elapsed() > Duration::from_millis(120) {
            dirty = true;
        }
        if dirty {
            {
                let g = gen.lock().unwrap();
                ui::draw(&mut p.fb, &status, &g, silent);
                if help {
                    ui::draw_help(&mut p.fb);
                }
            }
            p.flush();
            dirty = false;
            last_draw = Instant::now();
        }
        let Some(ev) = p.next_event(Some(Duration::from_millis(60))) else { continue };
        if ev.key == Key::Home && ev.state == KeyState::Press {
            break;
        }
        if !ev.is_down() || matches!(ev.key, Key::VolUp | Key::VolDown) {
            dirty = true;
            continue;
        }
        if help {
            help = false;
            dirty = true;
            continue;
        }
        let press = ev.state == KeyState::Press;
        let mut g = gen.lock().unwrap();
        let mut changed = true;
        match ev.key {
            Key::PlayPause if press => g.playing = !g.playing,
            Key::Up => g.tempo = (g.tempo + 4).min(TEMPO_MAX),
            Key::Down => g.tempo = g.tempo.saturating_sub(4).max(TEMPO_MIN),
            Key::Right if press => {
                g.scale = (g.scale + 1) % SCALES.len();
                g.regen();
            }
            Key::Left if press => {
                g.scale = (g.scale + SCALES.len() - 1) % SCALES.len();
                g.regen();
            }
            Key::Triangle if press => {
                g.root = (g.root + 7) % 12;
                g.regen();
            }
            Key::Circle | Key::Ok if press => {
                g.seed = g.seed % 9999 + 1;
                g.regen();
            }
            Key::Square if press => g.level = (g.level + 1) % LEVELS.len(),
            Key::Cross if press => {
                help = true;
                changed = false;
            }
            _ => changed = false,
        }
        if changed {
            save(&g);
        }
        dirty = true;
    }

    // fade out, end all notes, give the DAC back; MPD stays paused
    if let Ok(mut g) = gen.lock() {
        g.exit = true;
    }
    if let Some(a) = audio {
        let t = Instant::now();
        while a.alive() && t.elapsed() < Duration::from_millis(1500) {
            std::thread::sleep(Duration::from_millis(20));
        }
        a.shutdown();
    }
    if let Ok(g) = gen.lock() {
        save(&g);
    }
    eprintln!("tidepool: exit");
}

/// Headless check of the real audio path (no display): MPD pause, DAC via
/// aplay, generator + synth for `secs`, fade, exit. Prints frames, xruns,
/// notes and CPU use.
fn selftest(secs: u64) -> i32 {
    let gen = Arc::new(Mutex::new(load()));
    {
        let mut g = gen.lock().unwrap();
        g.playing = true;
    }
    let g2 = Arc::clone(&gen);
    let notes = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let n2 = Arc::clone(&notes);
    let mut patched = false;
    let a = match AudioRuntime::start_with(move |eng, frames| {
        if !patched {
            patched = true;
            eng.set_patch(Adsr { a: 0.35, d: 0.9, s: 0.55, r: 2.4 }, 1500.0, 0.12);
        }
        let mut g = g2.lock().unwrap();
        let before = g.tp.step_no;
        let r = g.drive(eng, frames);
        if g.tp.step_no != before {
            n2.store(g.tp.step_no, Ordering::Relaxed);
        }
        r
    }) {
        Ok(a) => a,
        Err(e) => {
            println!("selftest: FAIL audio: {e}");
            return 1;
        }
    };
    let t0 = Instant::now();
    std::thread::sleep(Duration::from_secs(secs));
    gen.lock().unwrap().exit = true;
    while a.alive() && t0.elapsed() < Duration::from_secs(secs + 3) {
        std::thread::sleep(Duration::from_millis(20));
    }
    let wall = t0.elapsed().as_secs_f32();
    let frames = a.frames_written.load(Ordering::Relaxed);
    let xruns = a.xruns.load(Ordering::Relaxed);
    let held = gen.lock().unwrap().tp.held_count();
    a.shutdown();
    let mut ru: libc::rusage = unsafe { std::mem::zeroed() };
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut ru) };
    let cpu = ru.ru_utime.tv_sec as f32 + ru.ru_utime.tv_usec as f32 / 1e6 + ru.ru_stime.tv_sec as f32 + ru.ru_stime.tv_usec as f32 / 1e6;
    let audio_s = frames as f32 / SAMPLE_RATE as f32;
    println!(
        "selftest: wall {wall:.1}s audio {audio_s:.1}s frames {frames} xruns {xruns} steps {} held_after {held} cpu {cpu:.2}s ({:.0}% of one core)",
        notes.load(Ordering::Relaxed),
        100.0 * cpu / wall
    );
    if xruns == 0 && held == 0 && audio_s > secs as f32 * 0.8 { 0 } else { 1 }
}
