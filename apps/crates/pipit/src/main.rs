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
use zpod_music_core::{AudioRuntime, FastArrowsGuard, PIPE_BYTES, SAMPLE_RATE};

/// Pipe into aplay: 16 KiB = ~93 ms instead of the default ~370 ms, so tempo,
/// tap, mute and audition respond quickly while keeping 5x the ALSA buffer
/// as scheduling headroom.
const PIPE: usize = 16384;
use zpod_seq::demos::DEMOS;
use zpod_seq::pattern::{Macros, Step};
use zpod_seq::{Clock, Event, EventBuf, EventKind, Pattern, Pitch};
use zpod_midi::router::{In, InEvent};
use zpod_midi::{MidiConfig, MidiOut, Router, State as MidiState, Status as MidiStatus};
use zpod_seq::event::EVENT_CAP;
use zpod_voices::{Bank, Rack, AUDITION};

/// How far the DAC lags the render: the aplay pipe (16 KiB = 92.9 ms at
/// 44.1 kHz stereo 16-bit, kept full by the blocking writer) plus the 768-frame
/// ALSA buffer (17.4 ms). MIDI is delayed by this much so it lines up with
/// the internal sound (estimate; not measured against external hardware).
const MIDI_LATENCY_NS: u64 = 110_000_000;

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
    /// one browser audition: (bank voice index, macros, pitch)
    audition: Option<(usize, Macros, Pitch)>,
    midi: MidiConfig,
    midi_ver: u64,
}

/// Audition note length.
const AUDITION_S: f32 = 0.7;

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
    /// audition note: id and frames until its note-off
    aud: Option<(u32, u32)>,
    aud_next: u32,
    /// MIDI out (None in benchmarks without MIDI)
    midi: Option<EngineMidi>,
    /// events for the rack when the internal sound is off (audition only)
    rack_ev: EventBuf,
    internal_was: bool,
}

struct EngineMidi {
    router: Router,
    status: Arc<MidiStatus>,
    epoch: Instant,
    ver: u64,
    inb: [InEvent; EVENT_CAP],
}

impl EngineMidi {
    fn new(out: &MidiOut, cfg: MidiConfig) -> EngineMidi {
        EngineMidi {
            router: Router::new(out.ring.clone(), cfg, SAMPLE_RATE, MIDI_LATENCY_NS),
            status: out.status.clone(),
            epoch: out.epoch(),
            ver: 0,
            inb: [InEvent { offset: 0, ev: In::AllOff }; EVENT_CAP],
        }
    }
}

impl Engine {
    fn new(bank: Bank, playhead: Arc<AtomicU32>, busy_ns: Arc<AtomicU64>) -> Self {
        Engine { clock: Clock::new(SAMPLE_RATE), rack: Rack::new(SAMPLE_RATE, bank), ev: EventBuf::new(), buf: vec![0.0; zpod_voices::MAX_BLOCK * 2], pat: Pattern::empty(), ver: u64::MAX, exiting: 0, playhead, busy_ns, aud: None, aud_next: 0, midi: None, rack_ev: EventBuf::new(), internal_was: true }
    }

    fn apply(&mut self, p: Pattern) {
        self.pat = p;
        self.rack.apply_pattern(&p, SAMPLE_RATE);
    }

    fn audition(&mut self, voice: usize, m: Macros, pitch: Pitch) {
        if let Some((id, _)) = self.aud.take() {
            self.ev.push(Event { offset: 0, kind: EventKind::NoteOff { track: AUDITION as u8, id } });
        }
        if self.rack.set_audition(voice, m) {
            self.aud_next = self.aud_next.wrapping_add(1);
            let id = 0x8000_0000 | self.aud_next; // never collides with clock ids
            self.ev.push(Event { offset: 0, kind: EventKind::NoteOn { track: AUDITION as u8, id, pitch, vel: 0.85 } });
            self.aud = Some((id, (AUDITION_S * SAMPLE_RATE as f32) as u32));
        }
    }

    /// One period. Returns false when the exit fade is done.
    fn period(&mut self, sh: &Mutex<Shared>, out: &mut [i16], frames: usize) -> bool {
        let t0 = Instant::now();
        self.ev.clear();
        let now_ns = self.midi.as_ref().map(|m| m.epoch.elapsed().as_nanos() as u64).unwrap_or(0);
        if let Ok(mut s) = sh.try_lock() {
            if s.version != self.ver {
                self.ver = s.version;
                self.apply(s.pat);
            }
            if let Some(m) = self.midi.as_mut() {
                if m.ver != s.midi_ver {
                    m.ver = s.midi_ver;
                    m.router.set_config(now_ns, s.midi);
                }
            }
            match s.transport.take() {
                Some(true) => self.clock.start(&self.pat),
                Some(false) => {
                    self.clock.stop(&mut self.ev);
                    self.aud = None; // AllNotesOff from stop ends it too
                }
                None => {}
            }
            if let Some((v, m, p)) = s.audition.take() {
                if !s.exit {
                    self.audition(v, m, p);
                }
            }
            self.rack.out_gain = if s.exit { 0.0 } else { s.out_gain };
            if s.exit && self.exiting == 0 {
                self.exiting = 1;
                if self.clock.playing() {
                    self.clock.stop(&mut self.ev);
                }
            }
        }
        if let Some((id, left)) = self.aud {
            if left <= frames as u32 {
                self.ev.push(Event { offset: left, kind: EventKind::NoteOff { track: AUDITION as u8, id } });
                self.aud = None;
            } else {
                self.aud = Some((id, left - frames as u32));
            }
        }
        self.clock.process(&self.pat, frames as u32, &mut self.ev);
        self.ev.sort();
        let mut internal = true;
        if let Some(m) = self.midi.as_mut() {
            // the audition always stays internal
            let mut n = 0;
            for e in self.ev.as_slice() {
                let ev = match e.kind {
                    EventKind::NoteOn { track, .. } | EventKind::NoteOff { track, .. } if track as usize == AUDITION => continue,
                    EventKind::NoteOn { track, id, pitch, vel } => In::On { lane: track, id, note: pitch.note(), vel },
                    EventKind::NoteOff { track, id } => In::Off { lane: track, id },
                    EventKind::AllNotesOff => In::AllOff,
                };
                m.inb[n] = InEvent { offset: e.offset, ev };
                n += 1;
            }
            m.router.block(now_ns, frames as u32, &m.inb[..n], self.pat.tempo, self.clock.playing());
            internal = m.router.config().internal_audible(m.status.state() == MidiState::Connected);
        }
        if internal {
            self.rack.render(self.ev.as_slice(), &mut self.buf, frames);
        } else {
            // MIDI only: keep the rack for the audition; end internal notes once
            self.rack_ev.clear();
            if self.internal_was {
                self.rack_ev.push(Event { offset: 0, kind: EventKind::AllNotesOff });
            }
            for e in self.ev.as_slice() {
                let keep = match e.kind {
                    EventKind::NoteOn { track, .. } | EventKind::NoteOff { track, .. } => track as usize == AUDITION,
                    EventKind::AllNotesOff => true,
                };
                if keep {
                    self.rack_ev.push(*e);
                }
            }
            self.rack.render(self.rack_ev.as_slice(), &mut self.buf, frames);
        }
        self.internal_was = internal;
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
    let pipe = std::env::var("PIPIT_PIPE_BYTES").ok().and_then(|v| v.parse().ok()).unwrap_or(PIPE);
    PIPE_BYTES.store(pipe, Ordering::Relaxed);
    match args.first().map(String::as_str) {
        Some("--help") => {
            eprintln!("pipit [--midi-test SECS [alsa] | --bench SECS [DEMO 1-5: cost per lane] | --selftest SECS [DEMO 1-5] | --xrun-test]");
            return;
        }
        Some("--bench") => std::process::exit(bench(
            args.get(1).and_then(|s| s.parse().ok()).unwrap_or(10.0),
            args.get(2).and_then(|s| s.parse::<usize>().ok()).map(|d| d.clamp(1, DEMOS.len()) - 1),
        )),
        Some("--selftest") => std::process::exit(selftest(
            args.get(1).and_then(|s| s.parse().ok()).unwrap_or(6).min(30),
            args.get(2).and_then(|s| s.parse::<usize>().ok()).unwrap_or(1).clamp(1, DEMOS.len()) - 1,
        )),
        Some("--xrun-test") => std::process::exit(xrun_test()),
        Some("--midi-test") => std::process::exit(midi_test(
            args.get(1).and_then(|s| s.parse().ok()).unwrap_or(6).clamp(2, 60),
            args.get(2).map(String::as_str) == Some("alsa"),
        )),
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

    let mut midi_note: Option<String> = None;
    // no file (first run) = Internal
    if let Ok(s) = std::fs::read_to_string(data_file("midi.toml")) {
        match MidiConfig::from_toml(&s, &editor::midi_default()) {
            Ok(c) => ed.midi = c,
            // keep Internal; the file is left alone until the user changes MIDI
            Err(e) => midi_note = Some(format!("midi.toml ignored ({e}); MIDI off")),
        }
    }
    if let Some(m) = &midi_note {
        eprintln!("pipit: {m}");
    }
    eprintln!("pipit: MIDI route {} port {:?}", ed.midi.route.label(), ed.midi.port);
    let mut midi_out = MidiOut::start(Box::new(zpod_midi::AlsaOpener::default()));
    midi_out.set_wanted(ed.midi.route.sends(), ed.midi.port);
    let shared = Arc::new(Mutex::new(Shared { pat: ed.pat, version: 0, transport: None, out_gain: OUTPUTS[ed.output].1, exit: false, audition: None, midi: ed.midi, midi_ver: 0 }));
    let playhead = Arc::new(AtomicU32::new(NO_STEP));
    let busy = Arc::new(AtomicU64::new(0));
    let audio = if p.is_sim() {
        None
    } else {
        let mut eng = Engine::new(bank, Arc::clone(&playhead), Arc::clone(&busy));
        eng.midi = Some(EngineMidi::new(&midi_out, ed.midi));
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
    let mut toast: Option<(String, Instant)> = midi_note.map(|m| (m, Instant::now()));
    let mut midi_ver = 0u64;
    let mut midi_poll = Instant::now() - Duration::from_secs(1);
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
        if midi_poll.elapsed() >= Duration::from_millis(300) {
            midi_poll = Instant::now();
            let (st, txt) = midi_status_line(&ed.midi, &midi_out);
            let ports = midi_out.status.ports.lock().map(|p| p.clone()).unwrap_or_default();
            if txt != ed.midi_status || ports != ed.midi_ports || st != ed.midi_connected {
                ed.midi_status = txt;
                ed.midi_ports = ports;
                ed.midi_connected = st;
                dirty = true;
            }
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
        ed.now_ms = started.elapsed().as_millis() as u64;
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
            Effect::Audition { voice, macros, pitched, octave } => {
                // tuned voices play the key's root in the lane's octave; drums at
                // their own pitch (plus the lane's pitch macro)
                let mut p = ed.pat;
                p.tracks[0].pitched = pitched;
                p.tracks[0].octave = octave;
                p.tracks[0].macros = macros;
                let pitch = p.pitch(0, &Step { on: true, ..Step::OFF });
                if silent {
                    toast = Some(("preview: no audio".into(), Instant::now()));
                } else if let Ok(mut s) = shared.lock() {
                    s.audition = Some((voice, macros, pitch));
                }
            }
            Effect::Midi { save } => {
                midi_ver += 1;
                midi_out.set_wanted(ed.midi.route.sends(), ed.midi.port);
                if let Ok(mut s) = shared.lock() {
                    s.midi = ed.midi;
                    s.midi_ver = midi_ver;
                }
                midi_poll = Instant::now() - Duration::from_secs(1);
                if save {
                    let r = std::fs::write(data_file("midi.toml"), ed.midi.to_toml());
                    toast = Some((
                        match r {
                            Ok(()) => format!("MIDI: {} (saved)", ed.midi.route.label()),
                            Err(e) => format!("MIDI not saved: {e}"),
                        },
                        Instant::now(),
                    ));
                }
            }
            Effect::Toast(m) => toast = Some((m, Instant::now())),
        }
    }
    if ed.midi_panel.is_some() {
        let _ = std::fs::write(data_file("midi.toml"), ed.midi.to_toml());
    }

    // Count write errors before the exit starts: the app manager stops an app
    // by signalling its whole process group, so aplay is already gone then
    // and later write errors are part of stopping, not underruns.
    let by_signal = STOP.load(Ordering::Relaxed);
    let run_errors = audio.as_ref().map(|a| a.xruns.load(Ordering::Relaxed)).unwrap_or(0);
    let underruns = audio.as_ref().map(|a| a.alsa_underruns.load(Ordering::Relaxed)).unwrap_or(0);
    let _ = write_pattern("current.toml", &ed.pat);
    if let Ok(mut s) = shared.lock() {
        s.exit = true;
    }
    if let Some(a) = audio {
        let t = Instant::now();
        // Home: let the fade play out. Signal: the output is closed, stop now.
        while !by_signal && a.alive() && t.elapsed() < Duration::from_millis(1500) {
            std::thread::sleep(Duration::from_millis(10));
        }
        let audio_s = a.frames_written.load(Ordering::Relaxed) as f64 / SAMPLE_RATE as f64;
        let cpu = busy.load(Ordering::Relaxed) as f64 / 1e9;
        eprintln!(
            "pipit: audio {:.1} s, ALSA underruns {}, write errors while running {}, render cpu {:.1} %{}",
            audio_s,
            underruns,
            run_errors,
            100.0 * cpu / audio_s.max(0.001),
            if by_signal { " (stopped by signal)" } else { "" }
        );
        a.shutdown();
    }
    // after the audio thread: its last messages (stop, panic) are queued
    midi_out.shutdown();
    let ms = &midi_out.status;
    if ed.midi.route.sends() {
        eprintln!(
            "pipit: MIDI sent {} unsent {} write errors {} late {} queue drops {} cleanups {}",
            ms.sent.load(Ordering::Relaxed),
            ms.unsent.load(Ordering::Relaxed),
            ms.write_errors.load(Ordering::Relaxed),
            ms.late.load(Ordering::Relaxed),
            midi_out.ring.dropped.load(Ordering::Relaxed),
            ms.cleanups.load(Ordering::Relaxed)
        );
    }
    eprintln!("pipit: exit after {:.0} s", started.elapsed().as_secs_f32());
}

/// (connected, one-line status) for the header and the MIDI setup.
fn midi_status_line(c: &MidiConfig, out: &MidiOut) -> (bool, String) {
    use zpod_midi::Fallback;
    let st = out.status.state();
    let connected = st == MidiState::Connected;
    let txt = match (c.route.sends(), st) {
        (false, _) => "MIDI off: internal sound".to_string(),
        (true, MidiState::Connected) => format!("MIDI to {}", out.status.detail()),
        (true, _) => {
            let why = out.status.detail();
            let why = if why.is_empty() { "searching".to_string() } else { why };
            match (c.route, c.fallback) {
                (zpod_midi::Route::Both, _) => format!("{why}: internal only"),
                (_, Fallback::Internal) => format!("{why}: playing internal"),
                (_, Fallback::Silent) => format!("{why}: SILENT"),
            }
        }
    };
    (connected, txt)
}

/// Render the densest demo offline (no audio device) and report CPU use.
fn bench(secs: f32, parts: Option<usize>) -> i32 {
    // same FPU mode as the real audio thread (PIPIT_BENCH_NO_FTZ=1 to compare)
    if std::env::var_os("PIPIT_BENCH_NO_FTZ").is_none() {
        zpod_music_core::flush_denormals();
    }
    let (bank, _) = load_bank();
    let mut worst = (0.0f64, String::new());
    let full_fx = || {
        let mut p = (DEMOS[0].build)();
        p.drive = 10;
        p.echo = 10;
        for t in p.tracks.iter_mut() {
            t.send = 10;
        }
        p
    };
    let runs: Vec<(String, Pattern)> = match parts {
        None => DEMOS.iter().map(|d| (d.name.to_string(), (d.build)())).chain([("ND full FX".to_string(), full_fx())]).collect(),
        // cost breakdown of one demo: whole, each lane alone, no echo, 1x rate
        Some(i) => {
            let base = (DEMOS[i].build)();
            let mut v = vec![(DEMOS[i].name.to_string(), base)];
            for t in 0..zpod_seq::pattern::TRACKS {
                let mut p = base;
                p.tracks[t].solo = true;
                v.push((format!("lane {t} {}", p.tracks[t].voice.as_str()), p));
            }
            let mut p = base;
            p.echo = 0;
            v.push(("no echo".into(), p));
            let mut p = base;
            p.rate = zpod_seq::Rate::Normal;
            v.push(("rate 1x".into(), p));
            v
        }
    };
    for (name, p) in runs {
        let name = name.as_str();
        let mut eng = Engine::new(bank.clone(), Arc::new(AtomicU32::new(0)), Arc::new(AtomicU64::new(0)));
        let sh = Mutex::new(Shared { pat: p, version: 1, transport: Some(true), out_gain: 1.0, exit: false, audition: None, midi: editor::midi_default(), midi_ver: 0 });
        let mut out = vec![0i16; 512];
        let n = (secs * SAMPLE_RATE as f32 / 256.0) as usize;
        let t0 = Instant::now();
        let mut voices = 0;
        for _ in 0..n {
            eng.period(&sh, &mut out, 256);
            voices = voices.max(eng.rack.active_voices());
        }
        let load = t0.elapsed().as_secs_f64() / (n as f64 * 256.0 / SAMPLE_RATE as f64);
        println!("bench {:12} {:5.1} % of one core, max voices {voices}, peak {:.2}", name, load * 100.0, eng.rack.peak);
        if load > worst.0 {
            worst = (load, name.to_string());
        }
    }
    let (wl, wn) = (worst.0, worst.1.to_string());
    println!("bench worst {wn} {:.1} %", wl * 100.0);
    if wl < 0.7 { 0 } else { 1 }
}

/// Real DAC path, no display: plays the first demo at the Soft level for
/// `secs`, stops, fades, exits. Prints frames, xruns, render CPU.
fn selftest(secs: u64, demo: usize) -> i32 {
    let (bank, src) = load_bank();
    let p = (DEMOS[demo].build)();
    let playhead = Arc::new(AtomicU32::new(NO_STEP));
    let busy = Arc::new(AtomicU64::new(0));
    let mut eng = Engine::new(bank, Arc::clone(&playhead), Arc::clone(&busy));
    let sh = Arc::new(Mutex::new(Shared { pat: p, version: 1, transport: Some(true), out_gain: OUTPUTS[0].1, exit: false, audition: None, midi: editor::midi_default(), midi_ver: 0 }));
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
    let under = a.alsa_underruns.load(Ordering::Relaxed);
    let audio_s = frames as f64 / SAMPLE_RATE as f64;
    let cpu = busy.load(Ordering::Relaxed) as f64 / 1e9;
    a.shutdown();
    println!(
        "selftest: demo '{}' bank {src}; audio {audio_s:.1} s frames {frames} ALSA underruns {under} pipe write errors {xruns} render cpu {:.1} % of one core",
        DEMOS[demo].name,
        100.0 * cpu / audio_s.max(0.001)
    );
    if xruns == 0 && under == 0 && audio_s > secs as f64 * 0.8 { 0 } else { 1 }
}

/// Checks that the underrun counter works: plays silence and stalls the
/// audio thread once for 1 s (longer than the pipe plus the 17 ms buffer). Passes when
/// aplay reports at least one underrun. Silent; ~2 s.
fn xrun_test() -> i32 {
    let mut n = 0u32;
    let a = match AudioRuntime::start_render(move |out, _| {
        out.iter_mut().for_each(|x| *x = 0);
        n += 1;
        if n == 100 {
            std::thread::sleep(Duration::from_millis(1000));
        }
        n < 350
    }) {
        Ok(a) => a,
        Err(e) => {
            println!("xrun-test: FAIL audio: {e}");
            return 1;
        }
    };
    let t0 = Instant::now();
    while a.alive() && t0.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(10));
    }
    std::thread::sleep(Duration::from_millis(100)); // let aplay's stderr arrive
    let under = a.alsa_underruns.load(Ordering::Relaxed);
    a.shutdown();
    println!("xrun-test: induced one 1 s stall; ALSA underruns counted: {under}");
    if under >= 1 { 0 } else { 1 }
}

/// MIDI routing test without sound: plays Night Drive in real time through
/// the engine (internal render included, output discarded) with MIDI routed
/// to a recording fake port, or with `alsa` to the real ALSA rawmidi ports
/// (`ZPOD_MIDI_PATH` may point at a FIFO). Mutes, solos and changes tempo
/// on the way; the fake port is unplugged and replugged. Then checks the
/// byte stream: every note ended, clock pulses present, no queue drops.
fn midi_test(secs: u64, alsa: bool) -> i32 {
    use zpod_midi::{FakeOpener, Route};
    zpod_music_core::flush_denormals(); // as on the real audio thread
    let (bank, _) = load_bank();
    let fake = FakeOpener::default();
    fake.present.store(true, Ordering::Relaxed);
    let mut out = if alsa { MidiOut::start(Box::new(zpod_midi::AlsaOpener::default())) } else { MidiOut::start(Box::new(fake.clone())) };
    let mut cfg = editor::midi_default();
    cfg.route = Route::Midi; // fallback Internal: exercised while unplugged
    cfg.clock = true;
    out.set_wanted(true, cfg.port);
    let t_wait = Instant::now();
    while !out.connected() && t_wait.elapsed() < Duration::from_millis(1500) {
        std::thread::sleep(Duration::from_millis(10));
    }
    println!("midi-test: port {:?} {}", out.status.state(), out.status.detail());
    let mut pat = (DEMOS[0].build)();
    let mut eng = Engine::new(bank, Arc::new(AtomicU32::new(0)), Arc::new(AtomicU64::new(0)));
    eng.midi = Some(EngineMidi::new(&out, editor::midi_default()));
    let sh = Mutex::new(Shared { pat, version: 1, transport: Some(true), out_gain: OUTPUTS[0].1, exit: false, audition: None, midi: cfg, midi_ver: 1 });
    let mut buf = vec![0i16; 512];
    let blocks = (secs as f64 * SAMPLE_RATE as f64 / 256.0) as u64;
    let period = Duration::from_nanos(256 * 1_000_000_000 / SAMPLE_RATE as u64);
    let t0 = Instant::now();
    let mut busy = Duration::ZERO;
    let (mut fallback_blocks, mut unplugged_blocks) = (0u32, 0u32);
    for b in 0..blocks + 40 {
        let f = b as f64 / blocks as f64;
        let mut edit = |g: &dyn Fn(&mut Pattern)| {
            g(&mut pat);
            if let Ok(mut s) = sh.lock() {
                s.pat = pat;
                s.version += 1;
            }
        };
        if b == blocks / 4 {
            edit(&|p| p.tracks[4].mute = true);
        }
        if b == blocks * 2 / 5 {
            edit(&|p| p.tracks[3].solo = true);
        }
        if b == blocks / 2 {
            edit(&|p| {
                p.tracks[3].solo = false;
                p.tracks[4].mute = false;
                p.tempo *= 1.5;
            });
        }
        if !alsa && b == blocks * 6 / 10 {
            fake.present.store(false, Ordering::Relaxed);
            fake.sink.gone.store(true, Ordering::Relaxed);
        }
        if !alsa && b == blocks * 3 / 4 {
            fake.present.store(true, Ordering::Relaxed);
        }
        if b == blocks {
            if let Ok(mut s) = sh.lock() {
                s.transport = Some(false);
            }
        }
        let _ = f;
        let t = Instant::now();
        eng.period(&sh, &mut buf, 256);
        busy += t.elapsed();
        if !out.connected() && b < blocks {
            unplugged_blocks += 1;
            if eng.internal_was {
                fallback_blocks += 1;
            }
        }
        // real time: messages are due ~110 ms after their block
        let due = t0 + period * (b as u32 + 1);
        if let Some(d) = due.checked_duration_since(Instant::now()) {
            std::thread::sleep(d);
        }
    }
    std::thread::sleep(Duration::from_millis(250));
    out.shutdown();
    let st = &out.status;
    let (sent, unsent, werr, late, drops, cleanups) = (
        st.sent.load(Ordering::Relaxed),
        st.unsent.load(Ordering::Relaxed),
        st.write_errors.load(Ordering::Relaxed),
        st.late.load(Ordering::Relaxed),
        out.ring.dropped.load(Ordering::Relaxed),
        st.cleanups.load(Ordering::Relaxed),
    );
    let el = t0.elapsed().as_secs_f64();
    println!(
        "midi-test: {:.1} s, engine cpu {:.1} % (render + routing), sent {sent} unsent {unsent} write errors {werr} late {late} queue drops {drops} cleanups {cleanups}",
        el,
        100.0 * busy.as_secs_f64() / el
    );
    println!("midi-test: blocks without a port {unplugged_blocks}, of which internal fallback played {fallback_blocks}");
    if alsa {
        println!("midi-test: real ALSA output; bytes are not checked here (no loopback)");
        return if drops == 0 { 0 } else { 1 };
    }
    // replay what the fake port received
    let bytes = fake.sink.bytes.lock().unwrap().clone();
    let mut on = [[0i32; 128]; 16];
    let (mut ons, mut clocks, mut starts, mut stops, mut i) = (0u64, 0u64, 0, 0, 0);
    while i < bytes.len() {
        let b0 = bytes[i];
        match b0 {
            0xF8 => clocks += 1,
            0xFA => starts += 1,
            0xFC => stops += 1,
            _ => {}
        }
        if b0 >= 0xF0 {
            i += 1;
            continue;
        }
        let (c, n) = ((b0 & 15) as usize, bytes[i + 1] as usize);
        match b0 & 0xf0 {
            0x90 => {
                ons += 1;
                on[c][n] += 1;
            }
            0x80 => on[c][n] = (on[c][n] - 1).max(0),
            0xB0 if n == 123 => on[c] = [0; 128],
            _ => {}
        }
        i += 3;
    }
    let hanging: i32 = on.iter().flatten().sum();
    println!("midi-test: fake port got {} bytes: note-ons {ons}, clock {clocks}, start {starts}, stop {stops}, hanging notes {hanging}", bytes.len());
    let ok = hanging == 0 && ons >= 4 * secs && clocks >= 15 * secs && starts >= 1 && stops >= 1 && drops == 0 && unplugged_blocks > 0 && fallback_blocks == unplugged_blocks;
    println!("midi-test: {}", if ok { "PASS (fake port: byte-level only, no external device)" } else { "FAIL" });
    if ok { 0 } else { 1 }
}
