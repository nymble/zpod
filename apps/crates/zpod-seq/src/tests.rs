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
    let bar = step_frames(44_100.0, 123.0) * 16.0;
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
    let sl = step_frames(44_100.0, p.tempo);
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
    let (a, b) = (step_frames(44_100.0, 100.0), step_frames(44_100.0, 150.0));
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
    let steps = 4000.0 * 256.0 / step_frames(44_100.0, p.tempo);
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
    let steps = 1000.0 * 256.0 / step_frames(44_100.0, p.tempo);
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
        good.replace("version = 1", "version = 9"),
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
