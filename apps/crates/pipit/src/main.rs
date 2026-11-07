//! Pipit for ZPOD: a 16-step, 5-track pocket sequencer (kick, snare, hat,
//! bass, lead) with synthesized voices. Design: docs/PIPIT-DESIGN.md,
//! manual: docs/PIPIT.md.
//!
//! Layers: editor.rs (buttons → edits, tested) · ui.rs (drawing) ·
//! zpod-seq (pattern, clock, generators) · zpod-voices (voice rack) ·
//! zpod-music-core (exclusive DAC, MPD pause). Starts stopped; Home saves,
//! fades and exits with MPD left paused.

mod editor;
mod ui;

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use editor::{Editor, Effect, OUTPUTS, SLOTS};
use zpod_core::StatusInfo;
use zpod_hal::Platform;
use zpod_music_core::{AudioRuntime, FastArrowsGuard, SAMPLE_RATE};
use zpod_seq::demos::DEMOS;
use zpod_seq::{Clock, EventBuf, Pattern};
use zpod_voices::{Bank, Rack};

static STOP: AtomicBool = AtomicBool::new(false);
extern "C" fn on_sigterm(_: libc::c_int) {
    STOP.store(true, Ordering::SeqCst);
}

/// UI → audio thread. The audio side only ever `try_lock`s it.
struct Shared {
    pat: Pattern,
    version: u64,
    transport: Option<bool>,
    out_gain: f32,
    exit: bool,
}

const NO_STEP: u32 = 255;

fn data_file(name: &str) -> std::path::PathBuf {
    zpod_hal::data_dir("pipit").join(name)
}

fn load_bank() -> (Bank, String) {
    let path = std::env::var_os("ZPOD_APP_DIR").map(|d| std::path::PathBuf::from(d).join("voices/default.toml"));
    if let Some(p) = path {
        match std::fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|s| Bank::parse(&s)) {
            Ok(b) => return (b, format!("{}", p.display())),
            Err(e) => eprintln!("pipit: {}: {e}; using the built-in bank", p.display()),
        }
    }
    (Bank::builtin(), "built-in".into())
}

fn read_pattern(name: &str) -> Result<Pattern, String> {
    let s = std::fs::read_to_string(data_file(name)).map_err(|e| e.to_string())?;
    Pattern::from_toml(&s)
}

fn write_pattern(name: &str, p: &Pattern) -> Result<(), String> {
    let path = data_file(name);
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, p.to_toml()).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

/// Bank-aware sanity: unknown voice ids fall back to the default kit.
fn fix_voices(p: &mut Pattern, bank: &Bank) {
    let d = Pattern::empty();
    for (t, tr) in p.tracks.iter_mut().enumerate() {
        if bank.find(tr.voice.as_str()).is_none() {
            eprintln!("pipit: unknown voice '{}' on track {}; using {}", tr.voice.as_str(), t + 1, d.tracks[t].voice.as_str());
            tr.voice = d.tracks[t].voice;
        }
    }
}

/// The audio callback state: clock + rack, fed from `Shared`.
struct Engine {
    clock: Clock,
    rack: Rack,
    ev: EventBuf,
    buf: Vec<f32>,
    pat: Pattern,
    ver: u64,
    exiting: u32,
    playhead: Arc<AtomicU32>,
    busy_ns: Arc<AtomicU64>,
}

impl Engine {
    fn new(bank: Bank, playhead: Arc<AtomicU32>, busy_ns: Arc<AtomicU64>) -> Self {
        Engine { clock: Clock::new(SAMPLE_RATE), rack: Rack::new(SAMPLE_RATE, bank), ev: EventBuf::new(), buf: vec![0.0; zpod_voices::MAX_BLOCK * 2], pat: Pattern::empty(), ver: u64::MAX, exiting: 0, playhead, busy_ns }
    }

    fn apply(&mut self, p: Pattern) {
        self.pat = p;
        for (t, tr) in p.tracks.iter().enumerate() {
            self.rack.set_track(t, tr.voice.as_str(), tr.level as f32 / 127.0, tr.mute);
        }
    }

    /// One period. Returns false when the exit fade is done.
    fn period(&mut self, sh: &Mutex<Shared>, out: &mut [i16], frames: usize) -> bool {
        let t0 = Instant::now();
        self.ev.clear();
        if let Ok(mut s) = sh.try_lock() {
            if s.version != self.ver {
                self.ver = s.version;
                self.apply(s.pat);
            }
            match s.transport.take() {
                Some(true) => self.clock.start(&self.pat),
                Some(false) => self.clock.stop(&mut self.ev),
                None => {}
            }
            self.rack.out_gain = if s.exit { 0.0 } else { s.out_gain };
            if s.exit && self.exiting == 0 {
                self.exiting = 1;
                if self.clock.playing() {
                    self.clock.stop(&mut self.ev);
                }
            }
        }
        self.clock.process(&self.pat, frames as u32, &mut self.ev);
        self.rack.render(self.ev.as_slice(), &mut self.buf, frames);
        for (o, x) in out.iter_mut().zip(self.buf[..frames * 2].iter()) {
            *o = (x * 32767.0) as i16;
        }
        self.playhead.store(self.clock.current_step.map(|s| s as u32).unwrap_or(NO_STEP), Ordering::Relaxed);
        self.busy_ns.fetch_add(t0.elapsed().as_nanos() as u64, Ordering::Relaxed);
        if self.exiting > 0 {
            self.exiting += 1;
            return self.exiting < 40; // ~230 ms: fade + release
        }
        true
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--help") => {
            eprintln!("pipit [--bench SECS | --selftest SECS]");
            return;
        }
        Some("--bench") => std::process::exit(bench(args.get(1).and_then(|s| s.parse().ok()).unwrap_or(10.0))),
        Some("--selftest") => std::process::exit(selftest(args.get(1).and_then(|s| s.parse().ok()).unwrap_or(6).min(30))),
        _ => {}
    }
    let mut p = match Platform::open() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("pipit: cannot open display: {e}");
            std::process::exit(1);
        }
    };
    unsafe {
        libc::signal(libc::SIGTERM, on_sigterm as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, on_sigterm as *const () as libc::sighandler_t);
    }
    let _fast = FastArrowsGuard::acquire();
    let (bank, bank_src) = load_bank();
    eprintln!("pipit: {} voices from {bank_src}", bank.voices.len());
    let mut pat = match read_pattern("current.toml") {
        Ok(p) => p,
        Err(e) => {
            eprintln!("pipit: no saved pattern ({e}); loading demo '{}'", DEMOS[0].name);
            (DEMOS[0].build)()
        }
    };
    fix_voices(&mut pat, &bank);
    let mut ed = Editor::new(pat, bank.clone());
    for i in 0..SLOTS {
        ed.slot_used[i as usize] = data_file(&format!("slot-{}.toml", i + 1)).exists();
    }
    if let Ok(s) = std::fs::read_to_string(data_file("output")) {
        ed.output = s.trim().parse::<usize>().unwrap_or(0).min(OUTPUTS.len() - 1);
    }

    let shared = Arc::new(Mutex::new(Shared { pat: ed.pat, version: 0, transport: None, out_gain: OUTPUTS[ed.output].1, exit: false }));
    let playhead = Arc::new(AtomicU32::new(NO_STEP));
    let busy = Arc::new(AtomicU64::new(0));
    let audio = if p.is_sim() {
        None
    } else {
        let mut eng = Engine::new(bank, Arc::clone(&playhead), Arc::clone(&busy));
        let sh = Arc::clone(&shared);
        match AudioRuntime::start_render(move |out, frames| eng.period(&sh, out, frames)) {
            Ok(a) => Some(a),
            Err(e) => {
                eprintln!("pipit: audio not available: {e}");
                None
            }
        }
    };
    let silent = audio.is_none();
    // simulator preview: run the clock without sound so the playhead moves
    let mut preview = if silent { Some((Clock::new(SAMPLE_RATE), EventBuf::new(), Instant::now())) } else { None };

    let mut version = 0u64;
    let mut status: StatusInfo = p.status.poll();
    let mut toast: Option<(String, Instant)> = None;
    let mut save_due: Option<Instant> = None;
    let mut last_head = NO_STEP;
    let mut dirty = true;
    let started = Instant::now();
    loop {
        if STOP.load(Ordering::Relaxed) {
            break;
        }
        if let Some((c, eb, t)) = preview.as_mut() {
            let n = (t.elapsed().as_secs_f64() * SAMPLE_RATE as f64) as u32;
            if n > 0 {
                *t = Instant::now();
                eb.clear();
                c.process(&ed.pat, n.min(1 << 20), eb);
                playhead.store(c.current_step.map(|s| s as u32).unwrap_or(NO_STEP), Ordering::Relaxed);
            }
        }
        let head = playhead.load(Ordering::Relaxed);
        if head != last_head {
            last_head = head;
            dirty = true;
        }
        let st = p.status.poll();
        if st != status {
            status = st;
            dirty = true;
        }
        if toast.as_ref().is_some_and(|(_, t)| t.elapsed() > Duration::from_millis(1600)) {
            toast = None;
            dirty = true;
        }
        if save_due.is_some_and(|t| Instant::now() >= t) {
            save_due = None;
            if let Err(e) = write_pattern("current.toml", &ed.pat) {
                eprintln!("pipit: autosave: {e}");
            }
        }
        if dirty {
            let ph = (head != NO_STEP && ed.playing).then_some(head as u8);
            ui::draw(&mut p.fb, &status, &ed, ph, toast.as_ref().map(|(s, _)| s.as_str()), silent);
            p.flush();
            dirty = false;
        }
        let Some(ev) = p.next_event(Some(Duration::from_millis(15))) else { continue };
        let fx = ed.key(ev.key, ev.state);
        dirty = true;
        match fx {
            Effect::None => {}
            Effect::Exit => break,
            Effect::Transport(on) => {
                if let Ok(mut s) = shared.lock() {
                    s.transport = Some(on);
                }
                if let Some((c, eb, t)) = preview.as_mut() {
                    *t = Instant::now();
                    if on { c.start(&ed.pat) } else { c.stop(eb) }
                }
            }
            Effect::Edited => {
                version += 1;
                if let Ok(mut s) = shared.lock() {
                    s.pat = ed.pat;
                    s.version = version;
                    s.out_gain = OUTPUTS[ed.output].1;
                }
                let _ = std::fs::write(data_file("output"), format!("{}\n", ed.output));
                save_due = Some(Instant::now() + Duration::from_secs(2));
            }
            Effect::SaveSlot(n) => {
                let msg = match write_pattern(&format!("slot-{n}.toml"), &ed.pat) {
                    Ok(()) => {
                        ed.slot_used[n as usize - 1] = true;
                        format!("Saved to slot {n}")
                    }
                    Err(e) => format!("Save failed: {e}"),
                };
                toast = Some((msg, Instant::now()));
            }
            Effect::LoadSlot(n) => {
                let msg = match read_pattern(&format!("slot-{n}.toml")) {
                    Ok(mut q) => {
                        fix_voices(&mut q, &ed.bank);
                        ed.replace(q);
                        version += 1;
                        if let Ok(mut s) = shared.lock() {
                            s.pat = ed.pat;
                            s.version = version;
                        }
                        save_due = Some(Instant::now() + Duration::from_secs(2));
                        format!("Loaded slot {n} (Undo restores)")
                    }
                    Err(e) => format!("Slot {n} not loaded: {e}"),
                };
                toast = Some((msg, Instant::now()));
            }
            Effect::Toast(m) => toast = Some((m, Instant::now())),
        }
    }

    let _ = write_pattern("current.toml", &ed.pat);
    if let Ok(mut s) = shared.lock() {
        s.exit = true;
    }
    if let Some(a) = audio {
        let t = Instant::now();
        while a.alive() && t.elapsed() < Duration::from_millis(1500) {
            std::thread::sleep(Duration::from_millis(10));
        }
        let audio_s = a.frames_written.load(Ordering::Relaxed) as f64 / SAMPLE_RATE as f64;
        let cpu = busy.load(Ordering::Relaxed) as f64 / 1e9;
        eprintln!("pipit: audio {:.1} s, xruns {}, render cpu {:.1} %", audio_s, a.xruns.load(Ordering::Relaxed), 100.0 * cpu / audio_s.max(0.001));
        a.shutdown();
    }
    eprintln!("pipit: exit after {:.0} s", started.elapsed().as_secs_f32());
}

/// Render the densest demo offline (no audio device) and report CPU use.
fn bench(secs: f32) -> i32 {
    let (bank, _) = load_bank();
    let mut worst = (0.0f64, "");
    for d in DEMOS {
        let p = (d.build)();
        let mut eng = Engine::new(bank.clone(), Arc::new(AtomicU32::new(0)), Arc::new(AtomicU64::new(0)));
        let sh = Mutex::new(Shared { pat: p, version: 1, transport: Some(true), out_gain: 1.0, exit: false });
        let mut out = vec![0i16; 512];
        let n = (secs * SAMPLE_RATE as f32 / 256.0) as usize;
        let t0 = Instant::now();
        let mut voices = 0;
        for _ in 0..n {
            eng.period(&sh, &mut out, 256);
            voices = voices.max(eng.rack.active_voices());
        }
        let load = t0.elapsed().as_secs_f64() / (n as f64 * 256.0 / SAMPLE_RATE as f64);
        println!("bench {:12} {:5.1} % of one core, max voices {voices}, peak {:.2}", d.name, load * 100.0, eng.rack.peak);
        if load > worst.0 {
            worst = (load, d.name);
        }
    }
    println!("bench worst {} {:.1} %", worst.1, worst.0 * 100.0);
    if worst.0 < 0.7 { 0 } else { 1 }
}

/// Real DAC path, no display: plays the first demo at the Soft level for
/// `secs`, stops, fades, exits. Prints frames, xruns, render CPU.
fn selftest(secs: u64) -> i32 {
    let (bank, src) = load_bank();
    let p = (DEMOS[0].build)();
    let playhead = Arc::new(AtomicU32::new(NO_STEP));
    let busy = Arc::new(AtomicU64::new(0));
    let mut eng = Engine::new(bank, Arc::clone(&playhead), Arc::clone(&busy));
    let sh = Arc::new(Mutex::new(Shared { pat: p, version: 1, transport: Some(true), out_gain: OUTPUTS[0].1, exit: false }));
    let sh2 = Arc::clone(&sh);
    let a = match AudioRuntime::start_render(move |out, frames| eng.period(&sh2, out, frames)) {
        Ok(a) => a,
        Err(e) => {
            println!("selftest: FAIL audio: {e}");
            return 1;
        }
    };
    let t0 = Instant::now();
    std::thread::sleep(Duration::from_secs(secs));
    sh.lock().unwrap().exit = true;
    while a.alive() && t0.elapsed() < Duration::from_secs(secs + 3) {
        std::thread::sleep(Duration::from_millis(10));
    }
    let frames = a.frames_written.load(Ordering::Relaxed);
    let xruns = a.xruns.load(Ordering::Relaxed);
    let audio_s = frames as f64 / SAMPLE_RATE as f64;
    let cpu = busy.load(Ordering::Relaxed) as f64 / 1e9;
    a.shutdown();
    println!("selftest: bank {src}; audio {audio_s:.1} s frames {frames} xruns {xruns} render cpu {:.1} % of one core", 100.0 * cpu / audio_s.max(0.001));
    if xruns == 0 && audio_s > secs as f64 * 0.8 { 0 } else { 1 }
}
