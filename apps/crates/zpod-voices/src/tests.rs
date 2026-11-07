use crate::def::{Bank, DEFAULT_BANK};
use crate::rack::{Rack, CEIL};
use zpod_seq::demos::DEMOS;
use zpod_seq::{Clock, Event, EventBuf, EventKind, Pitch};

const SR: u32 = 44_100;

#[test]
fn builtin_bank_valid() {
    let b = Bank::builtin();
    assert_eq!(b.voices.len(), 12);
    for d in DEMOS {
        let p = (d.build)();
        for t in p.tracks.iter() {
            assert!(b.find(t.voice.as_str()).is_some(), "{}: {}", d.name, t.voice.as_str());
        }
    }
}

#[test]
fn malformed_banks_rejected() {
    let bad = [
        DEFAULT_BANK.replace("format = \"pipit-voices\"", "format = \"x\""),
        DEFAULT_BANK.replace("version = 1", "version = 2"),
        DEFAULT_BANK.replacen("decay = 0.55", "decay = 99.0", 1),
        DEFAULT_BANK.replacen("kind = \"kick\"", "kind = \"theremin\"", 1),
        DEFAULT_BANK.replacen("drive = 0.25", "drive = 0.25\nwobble = 1.0", 1),
        DEFAULT_BANK.replacen("id = \"kick.tight\"", "id = \"kick.deep\"", 1),
        DEFAULT_BANK.replacen("id = \"clap\"", "id = \"bad id!\"", 1),
        DEFAULT_BANK.replacen("reso = 0.78", "reso = nan", 1),
        "format = \"pipit-voices\"\nversion = 1\nvoice = []\n".to_string(),
        "{{{".to_string(),
    ];
    for b in bad {
        assert!(Bank::parse(&b).is_err(), "accepted bad bank");
    }
}

fn one_voice(id: &str, gate_s: f32, total_s: f32) -> (Vec<f32>, Rack) {
    let mut r = Rack::new(SR, Bank::builtin());
    r.out_gain = 1.0;
    assert!(r.set_track(4, id, 1.0, false));
    let mut out = vec![0.0f32; 512];
    let mut all = Vec::new();
    let blocks = (total_s * SR as f32 / 256.0) as usize;
    let off_block = (gate_s * SR as f32 / 256.0) as usize;
    for b in 0..blocks {
        let mut ev = Vec::new();
        if b == 0 {
            ev.push(Event { offset: 10, kind: EventKind::NoteOn { track: 4, id: 7, pitch: Pitch::from_note(57.0), vel: 1.0 } });
        }
        if b == off_block {
            ev.push(Event { offset: 0, kind: EventKind::NoteOff { track: 4, id: 7 } });
        }
        r.render(&ev, &mut out, 256);
        all.extend_from_slice(&out);
    }
    (all, r)
}

#[test]
fn every_voice_sounds_ends_and_stays_clean() {
    let b = Bank::builtin();
    for v in &b.voices {
        let id = v.id.as_str();
        let (s, r) = one_voice(id, 0.25, 6.0);
        assert!(s.iter().all(|x| x.is_finite()), "{id}: NaN");
        let peak = s.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(peak > 0.05, "{id}: silent ({peak})");
        assert!(peak <= CEIL + 1e-3, "{id}: peak {peak}");
        let mean = s.iter().sum::<f32>() / s.len() as f32;
        assert!(mean.abs() < 0.01, "{id}: DC {mean}");
        assert_eq!(r.active_voices(), 0, "{id}: still sounding after 6 s");
        // no hard jumps after the attack (clicks): max step small vs peak
        let tail = &s[2 * (SR as usize / 2)..];
        let jump = tail.windows(2).fold(0.0f32, |m, w| m.max((w[1] - w[0]).abs()));
        assert!(jump < 0.5, "{id}: jump {jump}");
    }
}

#[test]
fn retrigger_and_steal_without_clicks() {
    for id in ["lead.pluck", "bass.acid", "kick.deep", "hat.open", "lead.bell"] {
        let mut r = Rack::new(SR, Bank::builtin());
        r.out_gain = 1.0;
        r.set_track(4, id, 1.0, false);
        let mut out = vec![0.0f32; 512];
        let mut prev = 0.0f32;
        let mut worst = 0.0f32;
        for b in 0..400u32 {
            let ev = [Event { offset: 37, kind: EventKind::NoteOn { track: 4, id: b + 1, pitch: Pitch::from_note(48.0 + (b % 12) as f32), vel: 1.0 } }];
            r.render(if b % 3 == 0 { &ev } else { &[] }, &mut out, 256);
            for fr in out.chunks(2) {
                worst = worst.max((fr[0] - prev).abs());
                prev = fr[0];
            }
        }
        assert!(worst < 0.9, "{id}: discontinuity {worst}");
        assert!(r.active_voices() <= 4);
    }
}

fn render_demo(idx: usize, secs: f32, block: usize) -> (Vec<f32>, Rack) {
    let p = (DEMOS[idx].build)();
    let mut r = Rack::new(SR, Bank::builtin());
    r.out_gain = 1.0;
    for (t, tr) in p.tracks.iter().enumerate() {
        r.set_track(t, tr.voice.as_str(), tr.level as f32 / 127.0, tr.mute);
    }
    let mut c = Clock::new(SR);
    c.start(&p);
    let mut eb = EventBuf::new();
    let mut out = vec![0.0f32; block * 2];
    let mut all = Vec::new();
    let n = (secs * SR as f32) as usize / block;
    for _ in 0..n {
        eb.clear();
        c.process(&p, block as u32, &mut eb);
        r.render(eb.as_slice(), &mut out, block);
        all.extend_from_slice(&out);
    }
    eb.clear();
    c.stop(&mut eb);
    r.render(eb.as_slice(), &mut out, block);
    for _ in 0..(4 * SR as usize / block) {
        r.render(&[], &mut out, block);
    }
    (all, r)
}

#[test]
fn demos_mix_with_headroom_and_stop_cleanly() {
    for i in 0..DEMOS.len() {
        let (s, r) = render_demo(i, 12.0, 256);
        let peak = s.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        let rms = (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt();
        let db = 20.0 * rms.log10();
        assert!(peak <= CEIL + 1e-3, "{}: peak {peak}", DEMOS[i].name);
        assert!((-30.0..-8.0).contains(&db), "{}: rms {db} dBFS", DEMOS[i].name);
        assert_eq!(r.overs, 0, "{}: overs", DEMOS[i].name);
        assert_eq!(r.active_voices(), 0, "{}: voices left after stop", DEMOS[i].name);
    }
}

#[test]
fn block_size_does_not_change_sound() {
    let (a, _) = render_demo(0, 3.0, 256);
    let (b, _) = render_demo(0, 3.0, 64);
    let n = a.len().min(b.len());
    let diff = a[..n].iter().zip(&b[..n]).fold(0.0f32, |m, (x, y)| m.max((x - y).abs()));
    assert!(diff < 1e-4, "diff {diff}");
}

#[test]
fn unknown_voice_keeps_previous_and_mute_silences() {
    let mut r = Rack::new(SR, Bank::builtin());
    assert!(r.set_track(0, "kick.deep", 1.0, false));
    assert!(!r.set_track(0, "nope", 1.0, false));
    assert_eq!(r.slots[0].def.id.as_str(), "kick.deep");
    r.set_track(0, "kick.deep", 1.0, true);
    let mut out = vec![0.0f32; 512];
    r.render(&[Event { offset: 0, kind: EventKind::NoteOn { track: 0, id: 1, pitch: Pitch::from_note(60.0), vel: 1.0 } }], &mut out, 256);
    assert!(out.iter().all(|x| *x == 0.0));
}


#[test]
fn packaged_bank_matches_builtin() {
    let pkg = include_str!("../../../packages/pipit/voices/default.toml");
    assert_eq!(pkg, DEFAULT_BANK, "apps/packages/pipit/voices/default.toml must equal banks/default.toml");
}
