use crate::clock::{step_frames, Clock};
use crate::demos::DEMOS;
use crate::event::{EventBuf, EventKind, Pitch};
use crate::gen;
use crate::pattern::*;

fn run(p: &Pattern, blocks: usize, block: u32) -> Vec<(u64, EventKind)> {
    let mut c = Clock::new(44_100);
    c.start(p);
    let mut out = Vec::new();
    let mut eb = EventBuf::new();
    for b in 0..blocks {
        eb.clear();
        c.process(p, block, &mut eb);
        assert_eq!(eb.dropped, 0);
        for e in eb.as_slice() {
            assert!(e.offset < block, "offset inside block");
            out.push((b as u64 * block as u64 + e.offset as u64, e.kind));
        }
    }
    out
}

#[test]
fn deterministic_and_block_size_independent() {
    for d in DEMOS {
        let p = (d.build)();
        let a = run(&p, 2000, 256);
        assert_eq!(a, run(&p, 2000, 256));
        // same absolute times with a different block size
        let b = run(&p, 2000 * 256 / 64, 64);
        assert_eq!(a, b, "{}", d.name);
    }
}

#[test]
fn step_times_exact_no_drift() {
    let mut p = Pattern::empty();
    p.tracks[0].steps[0] = Step { on: true, ..Step::OFF };
    p.tempo = 123.0;
    let ev = run(&p, 44_100 * 60 / 256, 256); // a minute
    let ons: Vec<u64> = ev.iter().filter(|e| matches!(e.1, EventKind::NoteOn { .. })).map(|e| e.0).collect();
    let bar = step_frames(44_100.0, 123.0, Rate::Normal) * 16.0;
    for (i, &t) in ons.iter().enumerate() {
        assert_eq!(t, (i as f64 * bar).round() as u64, "bar {i}");
    }
    assert!(ons.len() >= 30);
}

#[test]
fn swing_delays_odd_steps() {
    let mut p = Pattern::empty();
    for i in 0..16 {
        p.tracks[2].steps[i] = Step { on: true, ..Step::OFF };
    }
    p.swing = 66;
    let ev = run(&p, 200, 256);
    let ons: Vec<u64> = ev.iter().filter(|e| matches!(e.1, EventKind::NoteOn { .. })).map(|e| e.0).collect();
    let sl = step_frames(44_100.0, p.tempo, Rate::Normal);
    assert_eq!(ons[0], 0);
    assert_eq!(ons[1], (sl + 0.32 * sl).round() as u64);
    assert_eq!(ons[2], (2.0 * sl).round() as u64);
}

#[test]
fn every_note_ends_and_stop_releases_all() {
    for d in DEMOS {
        let p = (d.build)();
        let mut c = Clock::new(44_100);
        c.start(&p);
        let mut open = std::collections::HashSet::new();
        let mut eb = EventBuf::new();
        for _ in 0..3000 {
            eb.clear();
            c.process(&p, 256, &mut eb);
            for e in eb.as_slice() {
                match e.kind {
                    EventKind::NoteOn { id, track, pitch, vel } => {
                        assert!(open.insert(id));
                        assert!((track as usize) < TRACKS && pitch.hz > 20.0 && pitch.hz < 5000.0 && vel > 0.0 && vel <= 1.0);
                    }
                    EventKind::NoteOff { id, .. } => assert!(open.remove(&id), "off without on"),
                    _ => {}
                }
            }
            assert!(open.len() <= crate::clock::OFF_CAP);
        }
        eb.clear();
        c.stop(&mut eb);
        for e in eb.as_slice() {
            if let EventKind::NoteOff { id, .. } = e.kind {
                open.remove(&id);
            }
        }
        assert!(open.is_empty() && c.held() == 0, "{}: stuck notes", d.name);
        assert!(eb.as_slice().iter().any(|e| e.kind == EventKind::AllNotesOff));
        // stopped: nothing more
        eb.clear();
        c.process(&p, 256, &mut eb);
        assert!(eb.is_empty());
    }
}

#[test]
fn tempo_change_continues_smoothly() {
    let mut p = Pattern::empty();
    for i in 0..16 {
        p.tracks[2].steps[i] = Step { on: true, ..Step::OFF };
    }
    let mut c = Clock::new(44_100);
    c.start(&p);
    let mut eb = EventBuf::new();
    let mut t = Vec::new();
    for b in 0..400u64 {
        if b == 200 {
            p.tempo = 150.0;
        }
        eb.clear();
        c.process(&p, 256, &mut eb);
        t.extend(eb.as_slice().iter().filter(|e| matches!(e.kind, EventKind::NoteOn { .. })).map(|e| b * 256 + e.offset as u64));
    }
    let gaps: Vec<u64> = t.windows(2).map(|w| w[1] - w[0]).collect();
    let (a, b) = (step_frames(44_100.0, 100.0, Rate::Normal), step_frames(44_100.0, 150.0, Rate::Normal));
    assert!(gaps.iter().all(|&g| (g as f64 - a).abs() <= 1.0 || (g as f64 - b).abs() <= 1.0), "{gaps:?}");
}

#[test]
fn probability_is_seeded_and_roughly_right() {
    let mut p = Pattern::empty();
    for i in 0..16 {
        p.tracks[2].steps[i] = Step { on: true, prob: 50, ..Step::OFF };
    }
    let a = run(&p, 4000, 256);
    assert_eq!(a, run(&p, 4000, 256));
    let ons = a.iter().filter(|e| matches!(e.1, EventKind::NoteOn { .. })).count() as f64;
    let steps = 4000.0 * 256.0 / step_frames(44_100.0, p.tempo, Rate::Normal);
    assert!((ons / steps - 0.5).abs() < 0.08, "{}", ons / steps);
    p.seed = 2;
    assert_ne!(a, run(&p, 4000, 256));
}

#[test]
fn mute_and_length() {
    let mut p = (DEMOS[0].build)();
    for t in p.tracks.iter_mut() {
        t.mute = true;
    }
    assert!(run(&p, 500, 256).is_empty());
    let mut p = Pattern::empty();
    p.tracks[0].steps[0] = Step { on: true, ..Step::OFF };
    p.length = 4;
    let ons = run(&p, 1000, 256).iter().filter(|e| matches!(e.1, EventKind::NoteOn { .. })).count();
    let steps = 1000.0 * 256.0 / step_frames(44_100.0, p.tempo, Rate::Normal);
    assert!((ons as f64 - steps / 4.0).abs() <= 1.0);
}

#[test]
fn pitch_and_scales() {
    assert!((Pitch::from_note(69.0).hz - 440.0).abs() < 1e-3);
    assert!((Pitch::from_note(60.5).note() - 60.5).abs() < 1e-3);
    assert_eq!(Scale::Major.semis(7), 12);
    assert_eq!(Scale::Minor.semis(-1), -2);
    let p = Pattern::empty(); // A minor, bass octave 2
    let s = Step { on: true, note: 0, ..Step::OFF };
    assert!((p.pitch(3, &s).hz - 110.0).abs() < 0.01);
    assert_eq!(p.note_name(3, &s), "A2");
}

#[test]
fn file_roundtrip_and_validation() {
    for d in DEMOS {
        let p = (d.build)();
        let s = p.to_toml();
        assert_eq!(Pattern::from_toml(&s).unwrap(), p, "{}", d.name);
    }
    let good = (DEMOS[1].build)().to_toml();
    let bad = [
        good.replace("format = \"pipit-pattern\"", "format = \"other\""),
        good.replace("version = 2", "version = 9"),
        good.replace("rate = \"1/16\"", "rate = \"1/12\""),
        good.replacen("tone = 0", "tone = 11", 1),
        good.replacen("send = 0", "send = 11", 1),
        good.replace("echo_time = 1", "echo_time = 4"),
        good.replace("tempo = 122.0", "tempo = 900.0"),
        good.replace("swing = 52", "swing = 20"),
        good.replace("scale = \"minor\"", "scale = \"lydian-x\""),
        good.replacen("level = ", "bogus = 1\nlevel = ", 1),
        good.replacen("prob = [100, 100", "prob = [100, 101", 1),
        good.replacen("on = [", "on = [1, ", 1),
        "garbage {{".to_string(),
        String::new(),
    ];
    for b in bad {
        assert!(Pattern::from_toml(&b).is_err(), "accepted: {}", &b[..b.len().min(80)]);
    }
}

#[test]
fn generators_are_explicit_and_repeatable() {
    let p = (DEMOS[0].build)();
    let e = gen::euclid_fill(&p, 2, 5, 0);
    assert_eq!(e.tracks[2].steps.iter().filter(|s| s.on).count(), 5);
    assert_eq!(p, (DEMOS[0].build)(), "input untouched");
    let a = gen::walk_notes(&p, 3, 7);
    assert_eq!(a, gen::walk_notes(&p, 3, 7));
    assert_ne!(a.tracks[3].steps, gen::walk_notes(&p, 3, 8).tracks[3].steps);
    for (x, y) in a.tracks[3].steps.iter().zip(p.tracks[3].steps.iter()) {
        assert_eq!(x.on, y.on, "rhythm kept");
    }
    let c = gen::clear_track(&p, 4);
    assert!(c.tracks[4].steps.iter().all(|s| !s.on));
    for k in 0..=16u8 {
        assert_eq!((0..16).filter(|&i| gen::euclid(k, 16, 3, i)).count(), k as usize);
    }
}

fn ons(ev: &[(u64, EventKind)]) -> Vec<u64> {
    ev.iter().filter(|e| matches!(e.1, EventKind::NoteOn { .. })).map(|e| e.0).collect()
}

#[test]
fn rates_set_step_length_and_triplets_ignore_swing() {
    for r in Rate::ALL {
        let mut p = Pattern::empty();
        for i in 0..16 {
            p.tracks[2].steps[i] = Step { on: true, ..Step::OFF };
        }
        p.rate = r;
        p.swing = 66;
        let t = ons(&run(&p, 600, 256));
        let sl = 44_100.0 * 60.0 / (100.0 * r.steps_per_beat());
        assert_eq!(sl, step_frames(44_100.0, 100.0, r));
        // even steps are on the straight grid at every rate
        for (i, &x) in t.iter().enumerate().step_by(2) {
            assert_eq!(x, (i as f64 * sl).round() as u64, "{r:?} step {i}");
        }
        let odd = t[1] as f64 - sl;
        if r == Rate::Triplet {
            assert!(odd.abs() <= 1.0, "triplets are not swung");
        } else {
            assert!((odd - 0.32 * sl).abs() <= 1.0, "{r:?} swing");
        }
    }
}

#[test]
fn tempo_and_rate_changes_while_playing_keep_order_and_end_notes() {
    let mut p = (DEMOS[0].build)();
    let mut c = Clock::new(44_100);
    c.start(&p);
    let mut eb = EventBuf::new();
    let mut open = std::collections::HashSet::new();
    let mut last_on = 0u64;
    for b in 0..3000u64 {
        match b % 400 {
            100 => p.tempo = 180.0,
            200 => p.rate = Rate::Double,
            250 => p.rate = Rate::Triplet,
            300 => { p.tempo = 60.0; p.rate = Rate::Half }
            399 => { p.tempo = 104.0; p.rate = Rate::Normal }
            _ => {}
        }
        eb.clear();
        c.process(&p, 256, &mut eb);
        for e in eb.as_slice() {
            let at = b * 256 + e.offset as u64;
            match e.kind {
                EventKind::NoteOn { id, .. } => {
                    assert!(at >= last_on, "steps never go back in time");
                    last_on = at;
                    assert!(open.insert(id));
                }
                EventKind::NoteOff { id, .. } => assert!(open.remove(&id)),
                _ => {}
            }
        }
        // the longest gate is 4 steps of the slowest grid (60 bpm, 1/8): 2 s
        assert!(open.len() <= crate::clock::OFF_CAP);
    }
    eb.clear();
    c.stop(&mut eb);
    for e in eb.as_slice() {
        if let EventKind::NoteOff { id, .. } = e.kind {
            open.remove(&id);
        }
    }
    assert!(open.is_empty());
    // the gap at a tempo jump is between the old and the new step length
    let mut p = Pattern::empty();
    for i in 0..16 {
        p.tracks[2].steps[i] = Step { on: true, ..Step::OFF };
    }
    let mut c = Clock::new(44_100);
    c.start(&p);
    let mut t = Vec::new();
    for b in 0..400u64 {
        if b == 150 {
            p.rate = Rate::Double;
        }
        eb.clear();
        c.process(&p, 256, &mut eb);
        t.extend(eb.as_slice().iter().filter(|e| matches!(e.kind, EventKind::NoteOn { .. })).map(|e| b * 256 + e.offset as u64));
    }
    let (a, d) = (step_frames(44_100.0, 100.0, Rate::Normal), step_frames(44_100.0, 100.0, Rate::Double));
    for w in t.windows(2) {
        let g = (w[1] - w[0]) as f64;
        assert!(g >= d - 1.0 && g <= a + 1.0, "gap {g}");
    }
}

#[test]
fn solo_and_mute() {
    let mut p = (DEMOS[0].build)();
    let tracks_of = |p: &Pattern| -> std::collections::BTreeSet<u8> {
        run(p, 800, 256).iter().filter_map(|e| if let EventKind::NoteOn { track, .. } = e.1 { Some(track) } else { None }).collect()
    };
    assert_eq!(tracks_of(&p).len(), 5);
    p.tracks[1].solo = true;
    p.tracks[3].solo = true;
    assert_eq!(tracks_of(&p), [1u8, 3].into_iter().collect());
    assert!(!p.audible(0) && p.audible(1));
    p.tracks[3].mute = true; // mute wins over solo
    assert_eq!(tracks_of(&p), [1u8].into_iter().collect());
    p.tracks[1].solo = false;
    p.tracks[3].solo = false;
    assert_eq!(tracks_of(&p).len(), 4);
}

#[test]
fn muting_mid_note_still_ends_it() {
    let mut p = Pattern::empty();
    p.tracks[3].steps[0] = Step { on: true, gate: 16, ..Step::OFF };
    let mut c = Clock::new(44_100);
    c.start(&p);
    let mut eb = EventBuf::new();
    let mut on = 0;
    let mut off = 0;
    for b in 0..200 {
        if b == 2 {
            p.tracks[3].mute = true;
        }
        eb.clear();
        c.process(&p, 256, &mut eb);
        for e in eb.as_slice() {
            match e.kind {
                EventKind::NoteOn { .. } => on += 1,
                EventKind::NoteOff { .. } => off += 1,
                _ => {}
            }
        }
    }
    assert_eq!((on, off), (1, 1));
}

#[test]
fn macro_pitch_semitones_on_drums_cents_on_tuned() {
    let mut p = Pattern::empty();
    let s = Step { on: true, note: 0, ..Step::OFF };
    p.tracks[0].macros.pitch = 12;
    assert!((p.pitch(0, &s).hz - 2.0 * 261.6256).abs() < 0.05);
    p.tracks[3].macros.pitch = 50;
    assert!((p.pitch(3, &s).note() - 45.5).abs() < 1e-3, "A2 + 50 cents");
    p.tracks[3].macros.pitch = 90;
    p.clamp();
    assert_eq!(p.tracks[3].macros.pitch, TUNE_CENTS_MAX);
    p.tracks[0].macros.pitch = 40;
    p.clamp();
    assert_eq!(p.tracks[0].macros.pitch, DRUM_PITCH_MAX);
}

/// A file written by Pipit 0.1.x (format version 1, no rate/solo/macros/FX)
const V1: &str = include_str!("../testdata/pattern-v1.toml");

#[test]
fn version_1_files_still_load_with_defaults() {
    let p = Pattern::from_toml(V1).expect("v1 loads");
    assert_eq!(p.name.as_str(), "Night Drive");
    assert_eq!(p.rate, Rate::Normal);
    assert_eq!((p.drive, p.echo), (0, 0));
    assert!(p.tracks.iter().all(|t| !t.solo && t.macros == Macros::default() && t.send == 0));
    let d = (DEMOS[0].build)();
    for t in 0..TRACKS {
        assert_eq!(p.tracks[t].steps, d.tracks[t].steps, "steps kept, track {t}");
        assert_eq!(p.tracks[t].voice, d.tracks[t].voice);
    }
    // saving writes version 2, which reads back identically
    let s = p.to_toml();
    assert!(s.contains("version = 2"));
    assert_eq!(Pattern::from_toml(&s).unwrap(), p);
}

#[test]
fn version_2_roundtrip_with_everything_set() {
    let mut p = (DEMOS[2].build)();
    p.rate = Rate::Triplet;
    p.drive = 3;
    p.echo = 7;
    p.echo_time = 3;
    p.tracks[2].solo = true;
    p.tracks[4].macros = Macros { tone: -4, decay: 6, pitch: -20, drive: 5 };
    p.tracks[0].macros = Macros { tone: 10, decay: -10, pitch: -12, drive: 10 };
    p.tracks[4].send = 9;
    assert_eq!(Pattern::from_toml(&p.to_toml()).unwrap(), p);
    // a drum track may not carry a cents-sized pitch
    let bad = p.to_toml().replacen("pitch = -12", "pitch = -30", 1);
    assert!(Pattern::from_toml(&bad).is_err());
}
