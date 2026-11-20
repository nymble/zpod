//! Synth (Generative > Synth): a MIDI sound module. Plays MIDI input
//! through one zpod-voices voice. Inputs: every ALSA rawmidi port, read
//! only (a USB MIDI keyboard/controller with USB in host mode; a computer
//! over the USB MIDI gadget, Tools > USB > USB MIDI). Hot-plug every 2 s.
//!
//! Up/Down = voice (auditions C4), OK = audition, Left/Right = MIDI channel
//! (omni, 1-16), Square = panic (all notes off), Home = exit. Voice and
//! channel are saved to /data/zpod/appdata/synth/synth.conf.
//! `synth --render-test` renders a short MIDI phrase offline (no display,
//! no audio device) and checks it is audible and releases.

mod engine;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use engine::Engine;
use zpod_core::{Key, KeyState};
use zpod_hal::Platform;
use zpod_labui as ui;
use zpod_midi::input::{channel_ok, InMsg, Parser, RawMidiIn};
use zpod_midi::{AlsaOpener, Opener};
use zpod_music_core::{AudioRuntime, SAMPLE_RATE};
use zpod_ui::{text, text::Align, theme::*, Font};
use zpod_voices::Bank;

static STOP: AtomicBool = AtomicBool::new(false);
extern "C" fn on_sigterm(_: libc::c_int) {
    STOP.store(true, Ordering::SeqCst);
}

enum AMsg {
    Midi(InMsg),
    Voice(usize),
}

#[derive(Default)]
struct Status {
    ports: Vec<String>,
    last: Option<(u8, u8, u8)>,
    msgs: u64,
    errors: Vec<String>,
}

fn conf_path() -> PathBuf {
    ui::appdata("synth").join("synth.conf")
}

/// "voice = <id>" / "channel = <0-16>"
fn load_conf(bank: &Bank) -> (usize, u8) {
    let t = std::fs::read_to_string(conf_path()).unwrap_or_default();
    let mut voice = None;
    let mut ch = 0u8;
    for l in t.lines() {
        let Some((k, v)) = l.split_once('=') else { continue };
        let v = v.trim().trim_matches('"');
        match k.trim() {
            "voice" => voice = bank.find(v),
            "channel" => ch = v.parse().unwrap_or(0).min(16),
            _ => {}
        }
    }
    let first_tuned = (0..bank.voices.len()).find(|&i| !bank.voices[i].kind.is_drum()).unwrap_or(0);
    (voice.unwrap_or(first_tuned), ch)
}

fn save_conf(bank: &Bank, voice: usize, ch: u8) {
    let id = bank.voices.get(voice).map(|v| v.id.as_str().to_string()).unwrap_or_default();
    let _ = std::fs::write(conf_path(), format!("# ZPOD Synth settings\nvoice = \"{id}\"\nchannel = {ch}\n"));
}

fn note_name(n: u8) -> String {
    const N: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    format!("{}{}", N[(n % 12) as usize], n as i32 / 12 - 1)
}

/// MIDI input thread: (re)open every rawmidi port, poll, parse, forward.
fn midi_reader(tx: SyncSender<AMsg>, st: Arc<Mutex<Status>>, chan: Arc<AtomicU32>) {
    let mut opener = AlsaOpener::default();
    let mut ports: HashMap<PathBuf, RawMidiIn> = HashMap::new();
    let mut failed: HashMap<PathBuf, Instant> = HashMap::new();
    let mut next_scan = Instant::now();
    let mut buf = [0u8; 256];
    while !STOP.load(Ordering::Relaxed) {
        if Instant::now() >= next_scan {
            for p in opener.list() {
                if ports.contains_key(&p.path) || failed.get(&p.path).is_some_and(|t| t.elapsed() < Duration::from_secs(10)) {
                    continue;
                }
                match RawMidiIn::open(&p.path, &p.label()) {
                    Ok(r) => {
                        eprintln!("synth: MIDI in {} ({})", r.name, p.path.display());
                        ports.insert(p.path.clone(), r);
                    }
                    Err(e) => {
                        eprintln!("synth: {}: {e}", p.path.display());
                        failed.insert(p.path.clone(), Instant::now());
                    }
                }
            }
            if let Ok(mut s) = st.lock() {
                s.ports = ports.values().map(|r| r.name.clone()).collect();
                s.ports.sort();
            }
            next_scan = Instant::now() + Duration::from_secs(2);
        }
        if ports.is_empty() {
            std::thread::sleep(Duration::from_millis(100));
            continue;
        }
        let mut fds: Vec<libc::pollfd> = ports.values().map(|r| libc::pollfd { fd: r.raw_fd(), events: libc::POLLIN, revents: 0 }).collect();
        // SAFETY: poll(2) on our pollfd array
        unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, 50) };
        let mut gone = Vec::new();
        let filter = chan.load(Ordering::Relaxed) as u8;
        for (path, r) in ports.iter_mut() {
            loop {
                match r.read_some(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let mut got: Vec<InMsg> = Vec::new();
                        r.parser.feed_all(&buf[..n], |m| got.push(m));
                        for m in got {
                            if matches!(m, InMsg::Clock | InMsg::ActiveSensing) || !channel_ok(filter, &m) {
                                continue;
                            }
                            if let Ok(mut s) = st.lock() {
                                s.msgs += 1;
                                if let InMsg::NoteOn { ch, note, vel } = m {
                                    s.last = Some((ch, note, vel));
                                }
                            }
                            let _ = tx.try_send(AMsg::Midi(m));
                        }
                    }
                    Err(e) => {
                        eprintln!("synth: {} gone: {e}", r.name);
                        gone.push(path.clone());
                        break;
                    }
                }
            }
        }
        if !gone.is_empty() {
            for g in gone {
                ports.remove(&g);
            }
            let _ = tx.try_send(AMsg::Midi(InMsg::Cc { ch: 1, cc: 123, val: 0 }));
            next_scan = Instant::now();
        }
    }
}

fn render_test() -> i32 {
    let bank = Bank::builtin();
    let (voice, _) = (0..bank.voices.len()).find(|&i| !bank.voices[i].kind.is_drum()).map(|v| (v, 0)).unwrap_or((0, 0));
    let mut e = Engine::new(SAMPLE_RATE, bank, voice);
    let mut p = Parser::new();
    // C major arpeggio with running status, a bend, CC 123 at the end
    let phrase: &[&[u8]] = &[&[0x90, 60, 100], &[64, 100], &[67, 100], &[0xE0, 0, 0x50], &[0x80, 60, 0], &[0x90, 64, 0, 67, 0], &[0xB0, 123, 0]];
    let mut out = vec![0i16; 512];
    let t0 = Instant::now();
    let mut peaks = Vec::new();
    for chunk in phrase {
        p.feed_all(chunk, |m| e.handle(m));
        let mut pk = 0i16;
        for _ in 0..40 {
            e.render(&mut out, 256);
            pk = pk.max(out.iter().map(|x| x.saturating_abs()).max().unwrap_or(0));
        }
        peaks.push(pk);
    }
    let mut tail = 0i16;
    for k in 0..400 {
        e.render(&mut out, 256);
        if k >= 396 {
            tail = tail.max(out.iter().map(|x| x.saturating_abs()).max().unwrap_or(0));
        }
    }
    let audio_s = (phrase.len() * 40 + 400) as f64 * 256.0 / SAMPLE_RATE as f64;
    let cpu = t0.elapsed().as_secs_f64() / audio_s * 100.0;
    let ok = peaks[0] > 1000 && tail < 50;
    println!("synth render-test: voice {} peaks {:?} tail {} cpu {:.1}% of real time: {}", e.rack.bank.voices[voice].name.as_str(), peaks, tail, cpu, if ok { "PASS" } else { "FAIL" });
    if ok { 0 } else { 1 }
}

fn main() {
    if std::env::args().nth(1).as_deref() == Some("--render-test") {
        std::process::exit(render_test());
    }
    let mut p = match Platform::open() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("synth: cannot open display: {e}");
            std::process::exit(1);
        }
    };
    unsafe {
        libc::signal(libc::SIGTERM, on_sigterm as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, on_sigterm as *const () as libc::sighandler_t);
    }
    let bank = Bank::builtin();
    let (mut voice, ch0) = load_conf(&bank);
    let chan = Arc::new(AtomicU32::new(ch0 as u32));
    let status = Arc::new(Mutex::new(Status::default()));
    let (tx, rx): (SyncSender<AMsg>, Receiver<AMsg>) = sync_channel(512);
    let mut eng = Engine::new(SAMPLE_RATE, bank.clone(), voice);
    let audio = AudioRuntime::start_render(move |out, frames| {
        while let Ok(m) = rx.try_recv() {
            match m {
                AMsg::Midi(m) => eng.handle(m),
                AMsg::Voice(v) => eng.set_voice(v),
            }
        }
        eng.render(out, frames);
        !STOP.load(Ordering::Relaxed)
    });
    let audio = match audio {
        Ok(a) => Some(a),
        Err(e) => {
            eprintln!("synth: audio: {e}");
            status.lock().unwrap().errors.push(format!("Audio: {e}"));
            None
        }
    };
    {
        let (tx, st, ch) = (tx.clone(), status.clone(), chan.clone());
        std::thread::Builder::new().name("midi-in".into()).spawn(move || midi_reader(tx, st, ch)).expect("midi thread");
    }
    let mut aud_off: Option<Instant> = None;
    let mut note: Option<(String, Instant)> = None;
    let mut st = p.status.poll();
    let mut next_redraw = Instant::now();
    let mut dirty = true;
    let audition = |tx: &SyncSender<AMsg>| {
        let _ = tx.try_send(AMsg::Midi(InMsg::NoteOn { ch: 1, note: 60, vel: 100 }));
    };
    loop {
        if STOP.load(Ordering::Relaxed) {
            break;
        }
        if aud_off.is_some_and(|t| Instant::now() >= t) {
            let _ = tx.try_send(AMsg::Midi(InMsg::NoteOff { ch: 1, note: 60, vel: 64 }));
            aud_off = None;
        }
        if note.as_ref().is_some_and(|(_, t)| t.elapsed() > Duration::from_millis(1800)) {
            note = None;
            dirty = true;
        }
        let s2 = p.status.poll();
        if s2 != st {
            st = s2;
            dirty = true;
        }
        if dirty || Instant::now() >= next_redraw {
            let s = status.lock().map(|s| (s.ports.clone(), s.last, s.msgs, s.errors.clone())).unwrap_or_default();
            draw(&mut p.fb, &st, &bank, voice, chan.load(Ordering::Relaxed) as u8, &s.0, s.1, s.2, &s.3);
            if let Some((n, _)) = &note {
                ui::toast(&mut p.fb, n);
            }
            p.flush();
            dirty = false;
            next_redraw = Instant::now() + Duration::from_millis(250);
        }
        let Some(ev) = p.next_event(Some(Duration::from_millis(50))) else { continue };
        if ev.state == KeyState::Release {
            continue;
        }
        dirty = true;
        let n = bank.voices.len();
        match ev.key {
            Key::Home => break,
            Key::Up | Key::Down => {
                voice = if ev.key == Key::Down { (voice + 1) % n } else { (voice + n - 1) % n };
                let _ = tx.try_send(AMsg::Voice(voice));
                audition(&tx);
                aud_off = Some(Instant::now() + Duration::from_millis(450));
                save_conf(&bank, voice, chan.load(Ordering::Relaxed) as u8);
            }
            Key::Ok | Key::Circle => {
                audition(&tx);
                aud_off = Some(Instant::now() + Duration::from_millis(600));
            }
            Key::Left | Key::Right => {
                let c = chan.load(Ordering::Relaxed);
                let c = if ev.key == Key::Right { (c + 1) % 17 } else { (c + 16) % 17 };
                chan.store(c, Ordering::Relaxed);
                let _ = tx.try_send(AMsg::Midi(InMsg::Cc { ch: 1, cc: 123, val: 0 }));
                save_conf(&bank, voice, c as u8);
            }
            Key::Square => {
                let _ = tx.try_send(AMsg::Midi(InMsg::Cc { ch: 1, cc: 120, val: 0 }));
                note = Some(("All notes off".into(), Instant::now()));
            }
            _ => {}
        }
    }
    save_conf(&bank, voice, chan.load(Ordering::Relaxed) as u8);
    STOP.store(true, Ordering::SeqCst);
    if let Some(a) = audio {
        a.shutdown();
    }
    eprintln!("synth: exit");
}

#[allow(clippy::too_many_arguments)]
fn draw(fb: &mut zpod_ui::Framebuffer, st: &zpod_core::StatusInfo, bank: &Bank, voice: usize, ch: u8, ports: &[String], last: Option<(u8, u8, u8)>, msgs: u64, errors: &[String]) {
    let pill = if ports.is_empty() { "no MIDI in" } else { "MIDI in" };
    let info = if ports.is_empty() { "waiting for a port".to_string() } else { ports.join(", ") };
    ui::header(fb, st, "Synth", pill, if ports.is_empty() { ui::FAINT } else { ui::OK }, &info);
    let v = &bank.voices[voice];
    let cat = bank.info.get(voice).map(|i| i.category.clone()).unwrap_or_default();
    let desc = bank.info.get(voice).map(|i| i.desc.clone()).unwrap_or_default();
    text::draw_aligned(fb, Font::Small, &format!("Voice {} / {}  ·  {}", voice + 1, bank.voices.len(), cat), 160, 52, Align::Center, DIM);
    text::draw_aligned(fb, Font::Huge, v.name.as_str(), 160, 70, Align::Center, WHITE);
    text::draw_aligned(fb, Font::Small, &desc, 160, 102, Align::Center, DIM);
    let chs = if ch == 0 { "omni (all channels)".to_string() } else { format!("channel {ch} only") };
    let mut y = 128;
    let mut kv = |k: &str, val: &str, c| {
        text::draw(fb, Font::Small, k, 18, y + 2, DIM);
        text::draw_fit(fb, Font::Body, val, 90, y, 220, c);
        y += 19;
    };
    kv("MIDI", &chs, FG);
    match last {
        Some((c, n, vel)) => kv("Last", &format!("{} vel {vel}  ch {c}   ({msgs} msgs)", note_name(n)), CYAN),
        None => kv("Last", "no notes yet", DIM),
    }
    if ports.is_empty() {
        kv("Keyboard", "Tools > USB: devices", DIM);
        kv("Computer", "Tools > USB: USB MIDI on", DIM);
    }
    for e in errors.iter().take(1) {
        kv("Error", e, WARN);
    }
    ui::hints(fb, &[("Up/Dn", "voice"), ("OK", "play"), ("Lt/Rt", "channel"), ("Sq", "panic"), ("Home", "exit")]);
}
