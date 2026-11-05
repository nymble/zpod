//! Mouse Music for ZPOD — Music Mouse-style generative performance.
//!
//! Phase 1: splash, Play grid (4 harmonized voices), help overlay, one SUB
//! patch, MPD exclusive audio, fast D-pad arrows, `--bench`.
//! Input chooser after the splash: ZPOD buttons (always on) / USB mouse +
//! keyboard (zpod-input, hotplug; needs USB host mode) / Bluetooth (later).
//! Without a choice the highlighted one starts after a few seconds.
//!
//! Buttons (fast-arrows mode; Circle acts as OK):
//!   Lt/Rt lower voices · Up/Dn upper · Cir pattern (hold = menu soon)
//!   Tri mode / Tri+pad transpose|voicing · Sq tempo · X help
//!   Play start/stop · Home exit (fade, MPD output on, stay paused)

mod hidmap;
mod ui;

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use zpod_core::{Event, Key, KeyState, StatusInfo};
use zpod_input::{HidEvent, HidInput, SourceKind, UsbRole};
use zpod_hal::Platform;
use zpod_music_core::{
    bench_cpu, AudioRuntime, FastArrowsGuard, Pattern, PerfState, SharedPerf, Voicing, AXIS_STEPS,
};

use std::sync::atomic::{AtomicBool, Ordering};

static STOP: AtomicBool = AtomicBool::new(false);

extern "C" fn on_sigterm(_: libc::c_int) {
    STOP.store(true, Ordering::SeqCst);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Splash,
    Choose,
    Play,
    Help,
}

/// Chooser auto-start (buttons always work, so nothing ever blocks).
const CHOOSE_SECS: u32 = 6;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum InputMode {
    Buttons,
    Usb,
}

fn load_mode() -> InputMode {
    match std::fs::read_to_string(zpod_hal::data_dir("mousemusic").join("input")).as_deref().map(str::trim) {
        Ok("usb") => InputMode::Usb,
        _ => InputMode::Buttons,
    }
}

fn save_mode(m: InputMode) {
    let _ = std::fs::write(zpod_hal::data_dir("mousemusic").join("input"), if m == InputMode::Usb { "usb\n" } else { "buttons\n" });
}

/// Subtitle of the chooser's USB row (honest about the USB role).
fn usb_line(hid: &HidInput, role: UsbRole) -> (String, bool) {
    let devs = hid.devices();
    if let Some(d) = devs.first() {
        let more = if devs.len() > 1 { format!(" +{}", devs.len() - 1) } else { String::new() };
        return (format!("Found {} {}: {}{more}", d.bus_label(), d.kind.label(), d.name), true);
    }
    let s = match role {
        UsbRole::Gadget => "Needs host mode: USB is the Mac link now".to_string(),
        UsbRole::Host | UsbRole::Otg => "Plug in a mouse or keyboard".to_string(),
        UsbRole::Unknown => "No mouse or keyboard found".to_string(),
    };
    (s, false)
}

struct Hold {
    tri: Option<Instant>,
    sq: Option<Instant>,
    cir: Option<Instant>,
    shifted: bool,
}

impl Hold {
    fn new() -> Self {
        Self {
            tri: None,
            sq: None,
            cir: None,
            shifted: false,
        }
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    if let Some(a) = args.next() {
        if a == "--bench" {
            let voices: usize = args
                .next()
                .and_then(|s| s.parse().ok())
                .unwrap_or(4);
            let secs: f32 = args
                .next()
                .and_then(|s| s.parse().ok())
                .unwrap_or(2.0);
            let r = bench_cpu(secs, voices);
            println!("{}", r.report());
            return;
        }
        if a == "--help" || a == "-h" {
            eprintln!("mousemusic [--bench [voices] [seconds]]");
            return;
        }
    }

    let mut p = match Platform::open() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("mousemusic: cannot open display: {e}");
            std::process::exit(1);
        }
    };

    unsafe {
        libc::signal(libc::SIGTERM, on_sigterm as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, on_sigterm as *const () as libc::sighandler_t);
    }
    let _fast = FastArrowsGuard::acquire();

    let shared = SharedPerf::new(PerfState::default());
    let audio = match AudioRuntime::start(shared.clone_handle()) {
        Ok(a) => {
            eprintln!(
                "mousemusic: audio started (xruns will be logged; period 256 @ 44.1 kHz)"
            );
            Some(a)
        }
        Err(e) => {
            eprintln!("mousemusic: audio not available: {e}");
            None
        }
    };

    let hid = HidInput::spawn();
    let role = zpod_input::role::usb_role();
    eprintln!("mousemusic: USB role {}", role.label());
    let mut hidmap = hidmap::HidMap::default();
    let mut mode = InputMode::Buttons;
    let mut choose_sel = 0usize;
    let mut choose_until = Instant::now();

    let mut screen = Screen::Splash;
    #[allow(unused_assignments)]
    let mut status = StatusInfo::default();
    let mut trail: VecDeque<(i32, i32)> = VecDeque::new();
    trail.push_back((7, 7));
    let mut hold = Hold::new();
    let mut dirty = true;
    let mut toast: Option<(String, Instant)> = None;
    let mut last_chord_draw = Instant::now();

    // Initial splash
    {
        let st = p.status.poll();
        ui::draw_splash(&mut p.fb, &st);
        status = st;
        p.flush();
    }

    let exit_code = loop {
        let st = p.status.poll();
        if st != status {
            status = st;
            dirty = true;
        }

        // periodic redraw for arp dots / notes
        if screen == Screen::Play && last_chord_draw.elapsed() > Duration::from_millis(80) {
            dirty = true;
            last_chord_draw = Instant::now();
        }
        if let Some((_, t)) = &toast {
            if t.elapsed() > Duration::from_millis(1200) {
                toast = None;
                dirty = true;
            }
        }

        // USB / Bluetooth HID: hotplug always tracked; input used in USB mode
        while let Some(h) = hid.poll() {
            match h {
                HidEvent::Added(d) => {
                    if screen == Screen::Choose {
                        choose_sel = 1;
                        choose_until = Instant::now() + Duration::from_secs(CHOOSE_SECS as u64);
                    } else if mode == InputMode::Usb {
                        toast = Some((format!("{} connected", d.kind.label()), Instant::now()));
                    } else {
                        toast = Some((format!("USB {} found (buttons mode)", d.kind.label()), Instant::now()));
                    }
                    dirty = true;
                }
                HidEvent::Removed(d) => {
                    if mode == InputMode::Usb && screen != Screen::Choose {
                        let left = if hid.devices().is_empty() { ": buttons" } else { "" };
                        toast = Some((format!("{} removed{left}", d.kind.label()), Instant::now()));
                    }
                    dirty = true;
                }
                HidEvent::Input(e) => {
                    if mode != InputMode::Usb || !matches!(screen, Screen::Play | Screen::Help) {
                        continue;
                    }
                    if let Some(a) = hidmap.map(&e) {
                        if apply_action(a, &shared, &mut trail, &mut screen) {
                            dirty = true;
                        }
                    }
                }
            }
        }
        if screen == Screen::Choose {
            let left = choose_until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                mode = if choose_sel == 1 { InputMode::Usb } else { InputMode::Buttons };
                save_mode(mode);
                eprintln!("mousemusic: input {mode:?} (auto)");
                screen = Screen::Play;
            }
            dirty = true; // countdown
        }

        if dirty {
            let perf = shared.0.lock().unwrap().clone();
            let input_row = match mode {
                InputMode::Buttons => ("INPUT", "Buttons".to_string(), zpod_ui::theme::DIM),
                InputMode::Usb => {
                    let d = hid.devices();
                    let m = d.iter().any(|x| x.kind == SourceKind::Mouse);
                    let k = d.iter().any(|x| x.kind == SourceKind::Keyboard);
                    match (m, k) {
                        (true, true) => ("INPUT", "Mouse + keys".to_string(), zpod_ui::theme::OK),
                        (true, false) => ("INPUT", "USB mouse".to_string(), zpod_ui::theme::OK),
                        (false, true) => ("INPUT", "USB keys".to_string(), zpod_ui::theme::OK),
                        _ => ("INPUT", "USB: waiting".to_string(), zpod_ui::theme::ACCENT),
                    }
                }
            };
            match screen {
                Screen::Splash => ui::draw_splash(&mut p.fb, &status),
                Screen::Choose => {
                    let (line, found) = usb_line(&hid, role);
                    let secs = choose_until.saturating_duration_since(Instant::now()).as_millis().div_ceil(1000) as u32;
                    ui::draw_choose(&mut p.fb, &status, &ui::Chooser { sel: choose_sel, usb_line: &line, usb_found: found, countdown: Some(secs) });
                }
                Screen::Play => {
                    let t: Vec<_> = trail.iter().copied().collect();
                    ui::draw_play_input(&mut p.fb, &status, &perf, &t, (input_row.0, &input_row.1, input_row.2));
                }
                Screen::Help => {
                    let t: Vec<_> = trail.iter().copied().collect();
                    ui::draw_help(&mut p.fb, &status, &perf, &t, mode == InputMode::Usb && !hid.devices().is_empty());
                }
            }
            if let Some((msg, _)) = &toast {
                ui::draw_toast(&mut p.fb, msg);
            }
            p.flush();
            dirty = false;
        }

        let timeout = if screen == Screen::Play && mode == InputMode::Usb {
            Duration::from_millis(10) // HID events are polled between button waits
        } else if screen == Screen::Play {
            Duration::from_millis(40)
        } else {
            Duration::from_millis(200)
        };
        if STOP.load(Ordering::Relaxed) {
            break 0;
        }
        let Some(ev) = p.next_event(Some(timeout)) else {
            continue;
        };

        // Volume keys: informational only (zpod-buttons already applied).
        if matches!(ev.key, Key::VolUp | Key::VolDown) {
            dirty = true;
            continue;
        }

        if ev.key == Key::Home && ev.state == KeyState::Press {
            break 0;
        }

        match screen {
            Screen::Splash => {
                if ev.is_down() {
                    // preselect: a device is plugged in > last choice > buttons
                    choose_sel = if !hid.devices().is_empty() || load_mode() == InputMode::Usb { 1 } else { 0 };
                    choose_until = Instant::now() + Duration::from_secs(CHOOSE_SECS as u64);
                    screen = Screen::Choose;
                    dirty = true;
                }
            }
            Screen::Choose => {
                if ev.is_down() {
                    match ev.key {
                        Key::Up => choose_sel = choose_sel.saturating_sub(1),
                        Key::Down => choose_sel = (choose_sel + 1).min(1), // Bluetooth row is disabled
                        Key::Ok | Key::Circle | Key::Right | Key::PlayPause => {
                            mode = if choose_sel == 1 { InputMode::Usb } else { InputMode::Buttons };
                            save_mode(mode);
                            eprintln!("mousemusic: input {mode:?}");
                            screen = Screen::Play;
                        }
                        _ => {}
                    }
                    choose_until = Instant::now() + Duration::from_secs(CHOOSE_SECS as u64);
                    dirty = true;
                }
            }
            Screen::Help => {
                if ev.is_down() {
                    screen = Screen::Play;
                    dirty = true;
                }
            }
            Screen::Play => {
                if handle_play(
                    &ev,
                    &shared,
                    &mut hold,
                    &mut trail,
                    &mut screen,
                    &mut toast,
                ) {
                    dirty = true;
                }
            }
        }
    };

    // Fade and restore MPD (stay paused).
    {
        if let Ok(mut st) = shared.0.lock() {
            st.exit = true;
            st.fade = 1.0;
        }
    }
    if let Some(a) = audio {
        // give fade a moment inside the audio thread
        std::thread::sleep(Duration::from_millis(80));
        a.shutdown();
    }
    drop(_fast);
    eprintln!("mousemusic: exit {exit_code}");
    std::process::exit(exit_code);
}

/// One HID action on the performance state (true = redraw).
fn apply_action(a: hidmap::Action, shared: &SharedPerf, trail: &mut VecDeque<(i32, i32)>, screen: &mut Screen) -> bool {
    use hidmap::Action;
    if *screen == Screen::Help {
        // like the buttons: any action closes help (and is not applied)
        if matches!(a, Action::Move(..)) {
            return false;
        }
        *screen = Screen::Play;
        return true;
    }
    let mut st = shared.0.lock().unwrap();
    match a {
        Action::Move(dx, dy) => {
            st.x = (st.x + dx).clamp(0, AXIS_STEPS - 1);
            st.y = (st.y + dy).clamp(0, AXIS_STEPS - 1);
            st.chord = st.mapper.chord(st.x, st.y);
            let pos = (st.x, st.y);
            drop(st);
            if trail.back().copied() != Some(pos) {
                trail.push_back(pos);
                while trail.len() > 24 {
                    trail.pop_front();
                }
            }
        }
        Action::PlayToggle => st.sounding = !st.sounding,
        Action::Help => {
            drop(st);
            *screen = Screen::Help;
        }
        Action::Back => return false,
        Action::PatternToggle => st.pattern = st.pattern.toggle(),
        Action::ModeNext => {
            st.mapper.mode = st.mapper.mode.next();
            st.chord = st.mapper.chord(st.x, st.y);
        }
        Action::Mode(i) => {
            if let Some(m) = zpod_music_core::Mode::ALL.get(i) {
                st.mapper.mode = *m;
                st.chord = st.mapper.chord(st.x, st.y);
            }
        }
        Action::VoicingNext => {
            st.mapper.voicing = st.mapper.voicing.next();
            st.chord = st.mapper.chord(st.x, st.y);
        }
        Action::Transpose(d) => {
            st.mapper.transpose = (st.mapper.transpose + d).clamp(-12, 12);
            st.chord = st.mapper.chord(st.x, st.y);
        }
        Action::Tempo(d) => st.tempo_bpm = (st.tempo_bpm as i32 + d).clamp(40, 200) as u32,
    }
    true
}

fn handle_play(
    ev: &Event,
    shared: &SharedPerf,
    hold: &mut Hold,
    trail: &mut VecDeque<(i32, i32)>,
    screen: &mut Screen,
    toast: &mut Option<(String, Instant)>,
) -> bool {
    let mut dirty = false;
    let press = ev.state == KeyState::Press;
    let release = ev.state == KeyState::Release;
    let repeat = ev.state == KeyState::Repeat;
    let down = press || repeat;

    // Modifier holds
    match (ev.key, press, release) {
        (Key::Triangle, true, _) => {
            hold.tri = Some(Instant::now());
            hold.shifted = false;
        }
        (Key::Triangle, _, true) => {
            if let Some(t0) = hold.tri.take() {
                if !hold.shifted && t0.elapsed() < Duration::from_millis(450) {
                    let mut st = shared.0.lock().unwrap();
                    st.mapper.mode = st.mapper.mode.next();
                    st.chord = st.mapper.chord(st.x, st.y);
                    dirty = true;
                }
            }
            hold.shifted = false;
        }
        (Key::Square, true, _) => {
            hold.sq = Some(Instant::now());
            hold.shifted = false;
        }
        (Key::Square, _, true) => {
            if let Some(t0) = hold.sq.take() {
                if !hold.shifted && t0.elapsed() < Duration::from_millis(450) {
                    // tap Square: tempo nudge +4 as a quick action when no pad
                    let mut st = shared.0.lock().unwrap();
                    st.tempo_bpm = (st.tempo_bpm + 4).min(200);
                    dirty = true;
                }
            }
            hold.shifted = false;
        }
        (Key::Circle, true, _) => {
            hold.cir = Some(Instant::now());
            hold.shifted = false;
        }
        (Key::Circle, _, true) => {
            if let Some(t0) = hold.cir.take() {
                if !hold.shifted {
                    if t0.elapsed() >= Duration::from_millis(450) {
                        *toast = Some(("Menu coming soon".into(), Instant::now()));
                        dirty = true;
                    } else {
                        let mut st = shared.0.lock().unwrap();
                        st.pattern = st.pattern.toggle();
                        dirty = true;
                    }
                }
            }
            hold.shifted = false;
        }
        _ => {}
    }

    // Shift-chords while Tri/Sq held
    if down {
        if hold.tri.is_some() {
            match ev.key {
                Key::Up => {
                    let mut st = shared.0.lock().unwrap();
                    st.mapper.transpose = (st.mapper.transpose + 1).clamp(-12, 12);
                    st.chord = st.mapper.chord(st.x, st.y);
                    hold.shifted = true;
                    dirty = true;
                }
                Key::Down => {
                    let mut st = shared.0.lock().unwrap();
                    st.mapper.transpose = (st.mapper.transpose - 1).clamp(-12, 12);
                    st.chord = st.mapper.chord(st.x, st.y);
                    hold.shifted = true;
                    dirty = true;
                }
                Key::Left => {
                    let mut st = shared.0.lock().unwrap();
                    st.mapper.voicing = st.mapper.voicing.prev();
                    st.chord = st.mapper.chord(st.x, st.y);
                    hold.shifted = true;
                    dirty = true;
                }
                Key::Right => {
                    let mut st = shared.0.lock().unwrap();
                    st.mapper.voicing = st.mapper.voicing.next();
                    st.chord = st.mapper.chord(st.x, st.y);
                    hold.shifted = true;
                    dirty = true;
                }
                _ => {}
            }
            if hold.shifted {
                return dirty;
            }
        }
        if hold.sq.is_some() {
            match ev.key {
                Key::Up => {
                    let mut st = shared.0.lock().unwrap();
                    st.tempo_bpm = (st.tempo_bpm + 4).min(200);
                    hold.shifted = true;
                    dirty = true;
                    return dirty;
                }
                Key::Down => {
                    let mut st = shared.0.lock().unwrap();
                    st.tempo_bpm = st.tempo_bpm.saturating_sub(4).max(40);
                    hold.shifted = true;
                    dirty = true;
                    return dirty;
                }
                _ => {}
            }
        }
    }

    if down {
        match ev.key {
            Key::Left | Key::Right | Key::Up | Key::Down => {
                let mut st = shared.0.lock().unwrap();
                match ev.key {
                    Key::Left => st.x = (st.x - 1).max(0),
                    Key::Right => st.x = (st.x + 1).min(AXIS_STEPS - 1),
                    Key::Down => st.y = (st.y - 1).max(0),
                    Key::Up => st.y = (st.y + 1).min(AXIS_STEPS - 1),
                    _ => {}
                }
                st.chord = st.mapper.chord(st.x, st.y);
                let pos = (st.x, st.y);
                drop(st);
                if trail.back().copied() != Some(pos) {
                    trail.push_back(pos);
                    while trail.len() > 24 {
                        trail.pop_front();
                    }
                }
                dirty = true;
            }
            Key::Cross if press => {
                *screen = Screen::Help;
                dirty = true;
            }
            Key::PlayPause if press => {
                let mut st = shared.0.lock().unwrap();
                st.sounding = !st.sounding;
                dirty = true;
            }
            Key::Ok if press => {
                // Centre still works if buttons service emits it; treat as pattern too.
                let mut st = shared.0.lock().unwrap();
                st.pattern = st.pattern.toggle();
                dirty = true;
            }
            _ => {}
        }
    }
    let _ = Pattern::Off;
    let _ = Voicing::Parallel;
    dirty
}
