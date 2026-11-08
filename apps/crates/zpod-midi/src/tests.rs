use crate::config::*;
use crate::encode::*;
use crate::out::{MidiOut, State};
use crate::queue::{Item, Ring};
use crate::router::{In, InEvent, Router};
use crate::sink::FakeOpener;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

const SR: u32 = 44_100;

fn pipit_lanes() -> [Lane; 5] {
    [
        Lane { channel: 10, note: Some(36) },
        Lane { channel: 10, note: Some(38) },
        Lane { channel: 10, note: Some(42) },
        Lane { channel: 2, note: None },
        Lane { channel: 3, note: None },
    ]
}

fn cfg(route: Route) -> MidiConfig {
    let mut c = MidiConfig::new(&pipit_lanes());
    c.route = route;
    c
}

fn drain(r: &Ring) -> Vec<Item> {
    std::iter::from_fn(|| r.pop()).collect()
}

fn bytes(v: &[Item]) -> Vec<Vec<u8>> {
    v.iter().map(|i| i.msg.bytes().to_vec()).collect()
}

fn on(lane: u8, id: u32, note: f32) -> InEvent {
    InEvent { offset: 0, ev: In::On { lane, id, note, vel: 1.0 } }
}
fn off(lane: u8, id: u32) -> InEvent {
    InEvent { offset: 0, ev: In::Off { lane, id } }
}

#[test]
fn encoding_bytes() {
    assert_eq!(Msg::note_on(10, 36, 127).bytes(), &[0x99, 36, 127]);
    assert_eq!(Msg::note_on(1, 60, 0).bytes(), &[0x90, 60, 1], "velocity 0 would mean note-off");
    assert_eq!(Msg::note_off(16, 60).bytes(), &[0x8F, 60, 0]);
    assert_eq!(Msg::cc(2, 123, 0).bytes(), &[0xB1, 123, 0]);
    assert_eq!(Msg::bend(1, BEND_CENTER).bytes(), &[0xE0, 0x00, 0x40]);
    assert_eq!(Msg::bend(1, 16383).bytes(), &[0xE0, 0x7F, 0x7F]);
    assert_eq!(Msg::realtime(CLOCK).bytes(), &[0xF8]);
    assert_eq!(bend_value(0.0, 2), 8192);
    assert_eq!(bend_value(50.0, 2), 10240);
    assert_eq!(bend_value(-50.0, 2), 6144);
    assert_eq!(bend_value(-200.0, 2), 0);
    assert_eq!(bend_value(300.0, 2), 16383, "saturates past the range");
    assert_eq!(bend_value(50.0, 12), 8533);
    assert_eq!(vel7(0.0), 1);
    assert_eq!(vel7(1.0), 127);
    assert_eq!(vel7(f32::NAN), 100);
}

#[test]
fn config_defaults_internal_and_roundtrip() {
    let d = MidiConfig::new(&pipit_lanes());
    assert_eq!(d.route, Route::Internal);
    assert_eq!(d.fallback, Fallback::Internal);
    assert!(!d.clock);
    let mut c = d;
    c.route = Route::Both;
    c.clock = true;
    c.bend_range = 12;
    c.port = PortName::new("f_midi: MIDI function").unwrap();
    c.lanes[4] = Lane { channel: 16, note: None };
    let s = c.to_toml();
    assert_eq!(MidiConfig::from_toml(&s, &d).unwrap(), c);
    // auto port
    assert!(MidiConfig::from_toml(&d.to_toml(), &d).unwrap().port.is_auto());
}

#[test]
fn config_strict_validation() {
    let d = MidiConfig::new(&pipit_lanes());
    let good = d.to_toml();
    assert!(MidiConfig::from_toml(&good, &d).is_ok());
    let ch = "channels = [10, 10, 10, 2, 3]";
    let nt = "notes = [36, 38, 42, -1, -1]";
    assert!(good.contains(ch) && good.contains(nt), "{good}");
    let bad = [
        good.replace(ch, "channels = [0, 10, 10, 2, 3]"),
        good.replace(ch, "channels = [17, 10, 10, 2, 3]"),
        good.replace(ch, "channels = [10, 10, 2, 3]"),
        good.replace(ch, "channels = [10, 10, 10, 2, 3, 4]"),
        good.replace(nt, "notes = [128, 38, 42, -1, -1]"),
        good.replace(nt, "notes = [-2, 38, 42, -1, -1]"),
        good.replace("bend_range = 2", "bend_range = 0"),
        good.replace("bend_range = 2", "bend_range = 25"),
        good.replace("route = \"internal\"", "route = \"usb\""),
        good.replace("fallback = \"internal\"", "fallback = \"loud\""),
        good.replace("version = 1", "version = 2"),
        good.replace("format = \"zpod-midi\"", "format = \"other\""),
        good.replace("clock = false", "clock = false\nmpe = true"),
        good.replace("port = \"auto\"", &format!("port = \"{}\"", "x".repeat(40))),
        good.replace("clock = false\n", ""),
        "not toml at all [".to_string(),
    ];
    for (i, b) in bad.iter().enumerate() {
        assert_ne!(b, &good, "case {i} did not change the text");
        assert!(MidiConfig::from_toml(b, &d).is_err(), "case {i} accepted:\n{b}");
    }
}

#[test]
fn internal_route_sends_nothing() {
    let ring = Arc::new(Ring::new(64));
    let mut r = Router::new(ring.clone(), cfg(Route::Internal), SR, 0);
    r.block(0, 256, &[on(0, 1, 36.0), off(0, 1)], 120.0, true);
    assert!(ring.is_empty());
}

#[test]
fn overlap_and_retrigger_keep_note_lifetimes() {
    let ring = Arc::new(Ring::new(64));
    let mut r = Router::new(ring.clone(), cfg(Route::Midi), SR, 0);
    // two overlapping notes on the same channel/note: retrigger
    r.block(0, 256, &[on(3, 1, 45.0), on(3, 2, 45.0)], 120.0, true);
    assert_eq!(bytes(&drain(&ring)), vec![vec![0x91, 45, 127], vec![0x81, 45, 0], vec![0x91, 45, 127]]);
    // the first note's off must not cut the retriggered one
    r.block(0, 256, &[off(3, 1)], 120.0, true);
    assert!(ring.is_empty());
    r.block(0, 256, &[off(3, 2)], 120.0, true);
    assert_eq!(bytes(&drain(&ring)), vec![vec![0x81, 45, 0]]);
    // different notes overlap freely
    r.block(0, 256, &[on(4, 3, 60.0), on(4, 4, 64.0), off(4, 3), off(4, 4)], 120.0, true);
    assert_eq!(bytes(&drain(&ring)), vec![vec![0x92, 60, 127], vec![0x92, 64, 127], vec![0x82, 60, 0], vec![0x82, 64, 0]]);
    assert_eq!(r.sounding(), 0);
}

#[test]
fn drum_map_fixed_notes_and_cents_as_channel_bend() {
    let ring = Arc::new(Ring::new(64));
    let mut r = Router::new(ring.clone(), cfg(Route::Both), SR, 0);
    // drum lane: event pitch ignored
    r.block(0, 256, &[on(1, 1, 61.37)], 120.0, true);
    assert_eq!(bytes(&drain(&ring)), vec![vec![0x99, 38, 127]]);
    // tuned lane +25 cents: bend first, then the nearest note
    r.block(0, 256, &[on(4, 2, 60.25)], 120.0, true);
    let b = bend_value(25.0, 2);
    assert_eq!(bytes(&drain(&ring)), vec![vec![0xE2, (b & 0x7f) as u8, (b >> 7) as u8], vec![0x92, 60, 127]]);
    // same detune again: no repeated bend
    r.block(0, 256, &[on(4, 3, 62.25)], 120.0, true);
    assert_eq!(bytes(&drain(&ring)), vec![vec![0x92, 62, 127]]);
    // panic resets the bend on used channels
    r.panic(0);
    let v = bytes(&drain(&ring));
    assert!(v.contains(&vec![0x82, 60, 0]) && v.contains(&vec![0x82, 62, 0]));
    assert!(v.contains(&vec![0xE2, 0x00, 0x40]));
    assert!(v.contains(&vec![0xB9, 123, 0]) && v.contains(&vec![0xB1, 123, 0]) && v.contains(&vec![0xB2, 123, 0]));
}

#[test]
fn route_and_map_changes_panic_with_the_old_map() {
    let ring = Arc::new(Ring::new(64));
    let mut r = Router::new(ring.clone(), cfg(Route::Midi), SR, 0);
    r.block(0, 256, &[on(3, 1, 40.0)], 120.0, true);
    drain(&ring);
    let mut c = cfg(Route::Midi);
    c.lanes[3].channel = 5;
    r.set_config(0, c);
    let v = bytes(&drain(&ring));
    assert_eq!(v[0], vec![0x81, 40, 0], "old channel's note ended");
    assert!(v.contains(&vec![0xB1, 123, 0]));
    // the late note-off of the old note sends nothing now
    r.block(0, 256, &[off(3, 1)], 120.0, true);
    assert!(ring.is_empty());
    // to Internal: panic then silence
    r.block(0, 256, &[on(3, 2, 40.0)], 120.0, true);
    drain(&ring);
    r.set_config(0, cfg(Route::Internal));
    assert_eq!(bytes(&drain(&ring))[0], vec![0x84, 40, 0]);
    r.block(0, 256, &[on(3, 3, 40.0)], 120.0, true);
    assert!(ring.is_empty());
    // a clock-only change does not panic
    let mut c = cfg(Route::Midi);
    r.set_config(0, c);
    drain(&ring);
    r.block(0, 256, &[on(3, 4, 40.0)], 120.0, true);
    drain(&ring);
    c.clock = true;
    r.set_config(0, c);
    assert!(ring.is_empty());
}

#[test]
fn clock_is_opt_in_monotonic_and_follows_tempo() {
    let ring = Arc::new(Ring::new(4096));
    let mut r = Router::new(ring.clone(), cfg(Route::Midi), SR, 1_000_000);
    for i in 0..20 {
        r.block(i * 5_804_988, 256, &[], 120.0, true);
    }
    assert!(ring.is_empty(), "no clock unless enabled");
    let mut c = cfg(Route::Midi);
    c.clock = true;
    r.set_config(0, c);
    // 2 s at 120 bpm then 2 s at 90 bpm, block times from frame count
    let blocks = (4 * SR / 256) as u64;
    let ns = |b: u64| b * 256 * 1_000_000_000 / SR as u64;
    for b in 0..blocks {
        let t = if b < blocks / 2 { 120.0 } else { 90.0 };
        r.block(ns(b), 256, &[], t, true);
    }
    r.block(ns(blocks), 256, &[], 90.0, false);
    let v = drain(&ring);
    assert_eq!(v[0].msg.bytes(), &[START]);
    assert_eq!(v.last().unwrap().msg.bytes(), &[STOP]);
    let ticks: Vec<u64> = v.iter().filter(|i| i.msg.bytes() == [CLOCK]).map(|i| i.due_ns).collect();
    // 2 s * 48 + 2 s * 36 = 168 pulses (±2 at the edges)
    assert!((166..=170).contains(&ticks.len()), "{} pulses", ticks.len());
    assert!(ticks.windows(2).all(|w| w[1] > w[0]), "strictly monotonic");
    let first = ticks[1] - ticks[0];
    let last = ticks[ticks.len() - 1] - ticks[ticks.len() - 2];
    assert!((first as f64 - 20_833_333.0).abs() < 50_000.0, "120 bpm pulse {first}");
    assert!((last as f64 - 27_777_777.0).abs() < 50_000.0, "90 bpm pulse {last}");
    assert!(v.iter().all(|i| i.due_ns >= 1_000_000), "latency applied");
    // start again after stop: Start again
    r.block(ns(blocks + 1), 256, &[], 90.0, true);
    assert_eq!(drain(&ring)[0].msg.bytes(), &[START]);
    // disabling the clock while running sends Stop
    let c2 = cfg(Route::Midi);
    r.set_config(0, c2);
    assert!(drain(&ring).iter().any(|i| i.msg.bytes() == [STOP]));
}

#[test]
fn queue_bounded_drops_and_flags_lost() {
    let ring = Ring::new(8);
    for i in 0..10 {
        ring.push(Item { due_ns: i, msg: Msg::realtime(CLOCK) });
    }
    assert_eq!(ring.len(), 8);
    assert_eq!(ring.dropped.load(Ordering::Relaxed), 2);
    assert!(ring.lost.load(Ordering::Relaxed));
    let d: Vec<u64> = std::iter::from_fn(|| ring.pop()).map(|i| i.due_ns).collect();
    assert_eq!(d, (0..8).collect::<Vec<_>>(), "FIFO");
    // router counts overflow instead of blocking
    let ring = Arc::new(Ring::new(4));
    let mut r = Router::new(ring.clone(), cfg(Route::Midi), SR, 0);
    let evs: Vec<InEvent> = (0..10).map(|i| on(4, i, 50.0 + i as f32)).collect();
    r.block(0, 256, &evs, 120.0, true);
    assert!(r.overflow > 0);
}

#[test]
fn sequencer_mute_solo_and_stop_leave_no_hanging_notes() {
    use zpod_seq::demos::DEMOS;
    use zpod_seq::{Clock, EventBuf, EventKind};
    let ring = Arc::new(Ring::new(1 << 16));
    let mut c = cfg(Route::Midi);
    c.clock = true;
    let mut r = Router::new(ring.clone(), c, SR, 0);
    for d in DEMOS {
        let mut p = (d.build)();
        let mut clk = Clock::new(SR);
        clk.start(&p);
        let mut eb = EventBuf::new();
        let mut evs = Vec::new();
        for b in 0..2000u64 {
            if b == 400 {
                p.tracks[4].mute = true;
            }
            if b == 800 {
                p.tracks[3].solo = true;
            }
            if b == 1200 {
                p.tracks[3].solo = false;
                p.tracks[4].mute = false;
            }
            if b == 1500 {
                p.tempo *= 1.5;
            }
            eb.clear();
            if b == 1999 {
                clk.stop(&mut eb);
            } else {
                clk.process(&p, 256, &mut eb);
            }
            evs.clear();
            for e in eb.as_slice() {
                evs.push(InEvent {
                    offset: e.offset,
                    ev: match e.kind {
                        EventKind::NoteOn { track, id, pitch, vel } => In::On { lane: track, id, note: pitch.note(), vel },
                        EventKind::NoteOff { track, id } => In::Off { lane: track, id },
                        EventKind::AllNotesOff => In::AllOff,
                    },
                });
            }
            r.block(b * 5_804_988, 256, &evs, p.tempo, clk.playing());
        }
        assert_eq!(r.sounding(), 0, "{}", d.name);
    }
    // replay the byte stream: every note-on is ended
    let mut on = [[0i32; 128]; 16];
    let mut max_on = 0;
    for it in std::iter::from_fn(|| ring.pop()) {
        let b = it.msg.bytes();
        match b[0] & 0xf0 {
            0x90 => {
                on[(b[0] & 15) as usize][b[1] as usize] += 1;
                max_on = max_on.max(on[(b[0] & 15) as usize][b[1] as usize]);
            }
            0x80 => {
                let n = &mut on[(b[0] & 15) as usize][b[1] as usize];
                *n = (*n - 1).max(0);
            }
            _ => {}
        }
    }
    assert!(on.iter().flatten().all(|n| *n == 0), "hanging notes");
    assert_eq!(max_on, 1, "never two note-ons of the same note without an off");
    assert_eq!(ring.dropped.load(Ordering::Relaxed), 0);
}

fn wait_for(f: impl Fn() -> bool, ms: u64) -> bool {
    let t = Instant::now();
    while t.elapsed() < Duration::from_millis(ms) {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    f()
}

#[test]
fn worker_unavailable_hotplug_busy_unplug_and_shutdown_cleanup() {
    let fake = FakeOpener::default();
    let mut out = MidiOut::start(Box::new(fake.clone()));
    assert_eq!(out.status.state(), State::Off, "nothing opened while Internal");
    out.set_wanted(true, PortName::AUTO);
    assert!(wait_for(|| out.status.state() == State::Unavailable, 500), "no port -> Unavailable, not a fake Connected");
    assert!(!out.connected());
    // messages while unavailable are drained and counted, not queued forever
    out.ring.push(Item { due_ns: 0, msg: Msg::note_on(1, 60, 100) });
    assert!(wait_for(|| out.status.unsent.load(Ordering::Relaxed) == 1, 500));
    // plug in: found by the periodic rescan
    fake.present.store(true, Ordering::Relaxed);
    assert!(wait_for(|| out.connected(), 2500), "hotplug");
    let now = out.now_ns();
    out.ring.push(Item { due_ns: now, msg: Msg::note_on(2, 50, 90) });
    assert!(wait_for(|| fake.sink.bytes.lock().unwrap().ends_with(&[0x91, 50, 90]), 500));
    // busy port: retried, still delivered
    fake.sink.busy.store(3, Ordering::Relaxed);
    out.ring.push(Item { due_ns: out.now_ns(), msg: Msg::note_on(2, 52, 90) });
    assert!(wait_for(|| fake.sink.bytes.lock().unwrap().ends_with(&[0x91, 52, 90]), 500));
    // due times are honoured (not sent early)
    let due = out.now_ns() + 150_000_000;
    out.ring.push(Item { due_ns: due, msg: Msg::note_off(2, 52) });
    std::thread::sleep(Duration::from_millis(60));
    assert!(!fake.sink.bytes.lock().unwrap().ends_with(&[0x81, 52, 0]), "sent early");
    assert!(wait_for(|| fake.sink.bytes.lock().unwrap().ends_with(&[0x81, 52, 0]), 500));
    // unplug: write error -> Unavailable; replug -> reconnect + cleanup
    fake.present.store(false, Ordering::Relaxed);
    fake.sink.gone.store(true, Ordering::Relaxed);
    out.ring.push(Item { due_ns: out.now_ns(), msg: Msg::note_on(2, 55, 90) });
    assert!(wait_for(|| out.status.state() == State::Unavailable, 500));
    assert!(out.status.write_errors.load(Ordering::Relaxed) >= 1);
    fake.sink.bytes.lock().unwrap().clear();
    fake.present.store(true, Ordering::Relaxed);
    assert!(wait_for(|| out.connected(), 2500), "reconnect");
    assert!(wait_for(|| fake.sink.bytes.lock().unwrap().windows(3).any(|w| w == [0xB1, 123, 0]), 500), "reset after reconnect");
    // a sounding note at shutdown is ended
    fake.sink.bytes.lock().unwrap().clear();
    out.ring.push(Item { due_ns: out.now_ns(), msg: Msg::note_on(3, 70, 90) });
    assert!(wait_for(|| fake.sink.bytes.lock().unwrap().ends_with(&[0x92, 70, 90]), 500));
    out.shutdown();
    let b = fake.sink.bytes.lock().unwrap().clone();
    assert!(b.windows(3).any(|w| w == [0x82, 70, 0]), "note-off at shutdown: {b:?}");
    assert!(b.windows(3).any(|w| w == [0xB2, 123, 0]));
    assert_eq!(out.status.state(), State::Off);
}

#[test]
fn worker_cleans_up_after_queue_overflow_and_route_off() {
    let fake = FakeOpener::default();
    fake.present.store(true, Ordering::Relaxed);
    let out = MidiOut::start(Box::new(fake.clone()));
    out.set_wanted(true, PortName::AUTO);
    assert!(wait_for(|| out.connected(), 1500));
    out.ring.push(Item { due_ns: out.now_ns(), msg: Msg::note_on(4, 30, 90) });
    assert!(wait_for(|| fake.sink.bytes.lock().unwrap().ends_with(&[0x93, 30, 90]), 500));
    // simulate a lost message (overflow) -> cleanup ends the note
    out.ring.lost.store(true, Ordering::Release);
    assert!(wait_for(|| fake.sink.bytes.lock().unwrap().windows(3).any(|w| w == [0x83, 30, 0]), 500));
    assert!(out.status.cleanups.load(Ordering::Relaxed) >= 1);
    // route back to internal: port closed, state Off
    out.set_wanted(false, PortName::AUTO);
    assert!(wait_for(|| out.status.state() == State::Off, 500));
}

#[test]
fn chosen_port_must_match() {
    let fake = FakeOpener::default();
    fake.present.store(true, Ordering::Relaxed);
    let out = MidiOut::start(Box::new(fake.clone()));
    out.set_wanted(true, PortName::new("Some Synth").unwrap());
    assert!(wait_for(|| out.status.state() == State::Unavailable, 500));
    assert_eq!(out.status.detail(), "chosen port not found");
    out.set_wanted(true, PortName::new("fake").unwrap());
    assert!(wait_for(|| out.connected(), 1500));
}

#[test]
fn alsa_lister_reads_dev_and_proc() {
    let d = std::env::temp_dir().join(format!("zpod-midi-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("dev")).unwrap();
    std::fs::create_dir_all(d.join("proc/card1")).unwrap();
    std::fs::write(d.join("dev/midiC1D0"), b"").unwrap();
    std::fs::write(d.join("dev/pcmC0D0p"), b"").unwrap();
    std::fs::write(d.join("proc/card1/id"), b"Synth\n").unwrap();
    std::fs::write(d.join("proc/card1/midi0"), b"Synth MIDI 1\n\nOutput 0\n").unwrap();
    let mut o = crate::sink::AlsaOpener { dev_dir: d.join("dev"), proc_dir: d.join("proc"), override_path: None };
    let l = crate::sink::Opener::list(&mut o);
    assert_eq!(l.len(), 1);
    assert_eq!(l[0].id, "hw:1,0");
    assert_eq!(l[0].label(), "Synth: Synth MIDI 1");
    assert!(l[0].matches("auto") && l[0].matches("hw:1,0") && !l[0].matches("Other"));
    // a regular file opens and takes bytes (the real device is a char dev)
    let mut s = crate::sink::Opener::open(&mut o, &l[0]).unwrap();
    assert_eq!(s.write(&[0x90, 60, 100]), Ok(3));
    let _ = std::fs::remove_dir_all(&d);
}
