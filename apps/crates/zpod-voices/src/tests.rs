use crate::def::{Bank, DEFAULT_BANK};
use crate::dsp::{square, wrap};
use crate::rack::{shape, Rack, AUDITION, CEIL};
use zpod_seq::pattern::{Macros, Pattern};
use zpod_seq::demos::DEMOS;
use zpod_seq::{Clock, Event, EventBuf, EventKind, Pitch};

const SR: u32 = 44_100;

#[test]
fn builtin_bank_valid() {
    let b = Bank::builtin();
    assert_eq!(b.voices.len(), 20);
    assert_eq!(b.info.len(), 20);
    assert!(b.info.iter().all(|i| !i.desc.is_empty()), "every built-in voice has a description");
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
        DEFAULT_BANK.replace("version = 2", "version = 3"),
        DEFAULT_BANK.replace("version = 2", "version = 1"), // v1 cannot carry category/desc
        DEFAULT_BANK.replacen("category = \"kick\"", "category = \"Kick!\"", 1),
        DEFAULT_BANK.replacen("desc = \"round body, soft click, medium\"", "desc = \"this description is far too long for the screen\"", 1),
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
    assert_eq!(r.apply_pattern(&p, SR), 0);
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

#[test]
fn version_1_bank_without_categories_loads() {
    let v1: String = DEFAULT_BANK
        .lines()
        .filter(|l| !l.starts_with("category =") && !l.starts_with("desc ="))
        .map(|l| if l == "version = 2" { "version = 1" } else { l })
        .collect::<Vec<_>>()
        .join("\n");
    let b = Bank::parse(&v1).expect("v1 bank");
    let i = b.find("bass.acid").unwrap();
    assert_eq!(b.info[i].category, "bass", "category from the kind");
    assert_eq!(b.info[i].desc, "");
}

#[test]
fn browser_groups_by_category_and_family() {
    let b = Bank::builtin();
    let drums = b.browse(true);
    let tuned = b.browse(false);
    assert_eq!(drums.len() + tuned.len(), b.voices.len());
    assert!(drums.iter().all(|&i| b.voices[i].kind.is_drum()));
    assert!(tuned.iter().all(|&i| !b.voices[i].kind.is_drum()));
    // a category never appears in two separate runs
    for list in [&drums, &tuned] {
        let cats: Vec<&str> = list.iter().map(|&i| b.info[i].category.as_str()).collect();
        for w in cats.windows(2) {
            if w[0] != w[1] {
                assert!(!cats[..cats.iter().position(|c| *c == w[1]).unwrap()].contains(&w[1]));
            }
        }
    }
}

/// Spectral "brightness": RMS of the first difference over RMS (rises with
/// high-frequency content). A simple, deterministic stand-in for a centroid.
fn brightness(x: &[f32]) -> f32 {
    let e: f32 = x.iter().map(|v| v * v).sum();
    let d: f32 = x.windows(2).map(|w| (w[1] - w[0]) * (w[1] - w[0])).sum();
    (d / e.max(1e-12)).sqrt()
}

fn hit(id: &str, vel: f32, m: Macros, secs: f32) -> Vec<f32> {
    let mut r = Rack::new(SR, Bank::builtin());
    r.out_gain = 1.0;
    r.set_track(0, id, 1.0, false);
    r.slots[0].macros = m;
    let mut out = vec![0.0f32; 512];
    let mut all = Vec::new();
    for b in 0..(secs * SR as f32 / 256.0) as usize {
        let ev = [Event { offset: 0, kind: EventKind::NoteOn { track: 0, id: 1, pitch: Pitch::from_note(60.0), vel } }];
        r.render(if b == 0 { &ev } else { &[] }, &mut out, 256);
        all.extend(out.chunks(2).map(|f| f[0] + f[1]));
    }
    all
}

fn length_s(x: &[f32]) -> f32 {
    let pk = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    let last = x.iter().rposition(|v| v.abs() > pk * 0.01).unwrap_or(0);
    last as f32 / SR as f32
}

#[test]
fn velocity_changes_drum_timbre_not_only_level() {
    for id in ["kick.deep", "snare.crack", "hat.closed", "clap"] {
        let soft = hit(id, 0.3, Macros::default(), 1.5);
        let hard = hit(id, 1.0, Macros::default(), 1.5);
        let (bs, bh) = (brightness(&soft), brightness(&hard));
        println!("velocity brightness {id}: soft {bs:.3} hard {bh:.3}");
        assert!(bh > bs * 1.03, "{id}: hard {bh} vs soft {bs} not brighter");
    }
}

#[test]
fn macros_move_tone_and_length_within_bounds() {
    let b = Bank::builtin();
    for v in &b.voices {
        for m in [Macros { tone: 10, decay: 10, pitch: 0, drive: 10 }, Macros { tone: -10, decay: -10, pitch: 0, drive: 0 }] {
            let d = shape(*v, m);
            for &(name, _, lo, hi) in crate::def::FIELDS {
                let x = match name { "cutoff" => d.cutoff, "tone" => d.tone, "decay" => d.decay, "release" => d.release, "drive" => d.drive, _ => continue };
                assert!((lo..=hi).contains(&x), "{} {name} {x}", v.id.as_str());
            }
        }
    }
    for id in ["kick.deep", "hat.open", "snare.soft"] {
        let short = hit(id, 1.0, Macros { decay: -8, ..Macros::default() }, 3.0);
        let long = hit(id, 1.0, Macros { decay: 8, ..Macros::default() }, 3.0);
        assert!(length_s(&long) > 1.5 * length_s(&short), "{id}: decay macro");
        let dark = hit(id, 1.0, Macros { tone: -8, ..Macros::default() }, 1.0);
        let bright = hit(id, 1.0, Macros { tone: 8, ..Macros::default() }, 1.0);
        assert!(brightness(&bright) > brightness(&dark), "{id}: tone macro");
        for x in [&short, &long, &dark, &bright] {
            assert!(x.iter().all(|v| v.is_finite() && v.abs() <= 2.0 * CEIL));
        }
    }
}

/// Power spectrum of a Hann-windowed block at bins 0..n/2 (plain DFT; test only).
fn spectrum(x: &[f32]) -> Vec<f64> {
    let n = x.len();
    let w: Vec<f64> = (0..n).map(|i| x[i] as f64 * (0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / n as f64).cos())).collect();
    (0..n / 2)
        .map(|k| {
            let (mut re, mut im) = (0.0, 0.0);
            let step = std::f64::consts::TAU * k as f64 / n as f64;
            for (i, v) in w.iter().enumerate() {
                let a = step * i as f64;
                re += v * a.cos();
                im -= v * a.sin();
            }
            re * re + im * im
        })
        .collect()
}

/// Energy (dB) of everything that is not a true harmonic of f0, relative to
/// the harmonics: the aliasing of a square oscillator.
pub fn alias_db(f0: f64, blep: bool) -> f64 {
    let n = 4096;
    let sr = SR as f64;
    let dt = (f0 / sr) as f32;
    let mut ph = 0.0f32;
    let x: Vec<f32> = (0..n)
        .map(|_| {
            ph = wrap(ph + dt);
            if blep { square(ph, dt) } else if ph < 0.5 { 1.0 } else { -1.0 }
        })
        .collect();
    let sp = spectrum(&x);
    let bin = sr / n as f64;
    let (mut harm, mut alias) = (0.0, 0.0);
    for (k, p) in sp.iter().enumerate().skip(2) {
        let f = k as f64 * bin;
        let h = (f / f0).round();
        let near = h >= 1.0 && (f - h * f0).abs() <= 3.0 * bin && (h as i64) % 2 == 1;
        if near { harm += p } else { alias += p }
    }
    10.0 * (alias / harm).log10()
}

#[test]
fn band_limited_hat_partials_alias_far_less_than_naive() {
    // the highest hat partial at default pitch (410 Hz × 2.66) and an octave up
    for f0 in [1092.0, 2184.0] {
        let (naive, bl) = (alias_db(f0, false), alias_db(f0, true));
        println!("hat alias f0 {f0} Hz: polyBLEP {bl:.1} dB, naive {naive:.1} dB");
        assert!(bl < naive - 10.0, "f0 {f0}: polyBLEP {bl:.1} dB vs naive {naive:.1} dB");
    }
}

#[test]
fn echo_repeats_at_the_set_time_and_dies_away() {
    let mut r = Rack::new(SR, Bank::builtin());
    r.out_gain = 1.0;
    r.set_track(1, "snare.rim", 1.0, false);
    r.slots[1].send = 1.0;
    r.set_echo(0.6, 11_025);
    let mut out = vec![0.0f32; 512];
    let mut all = Vec::new();
    for b in 0..(5 * SR as usize / 256) {
        let ev = [Event { offset: 0, kind: EventKind::NoteOn { track: 1, id: 1, pitch: Pitch::from_note(60.0), vel: 1.0 } }];
        // after the fade-in has settled
        r.render(if b == 300 { &ev } else { &[] }, &mut out, 256);
        all.extend(out.chunks(2).map(|f| f[0].abs() + f[1].abs()));
    }
    let t0 = 300 * 256;
    let win = |a: usize| all[t0 + a..t0 + a + 600].iter().fold(0.0f32, |m, v| m.max(*v));
    let dry = win(0);
    let rep1 = win(11_025);
    let rep2 = win(22_050);
    let gap = win(5_500);
    assert!(rep1 > 0.1 * dry && rep1 < dry, "first repeat {rep1} vs dry {dry}");
    assert!(rep2 < rep1, "feedback decays");
    assert!(gap < 0.2 * rep1, "nothing between repeats ({gap})");
    assert!(all[all.len() - 2000..].iter().all(|v| *v < 1e-3), "echo dies away");
    assert!(all.iter().all(|v| v.is_finite()));
}

#[test]
fn drive_and_echo_keep_the_ceiling_and_stop_cleanly() {
    for i in 0..DEMOS.len() {
        let mut p = (DEMOS[i].build)();
        p.drive = 10;
        p.echo = 10;
        for t in p.tracks.iter_mut() {
            t.send = 10;
        }
        let mut r = Rack::new(SR, Bank::builtin());
        r.out_gain = 1.0;
        r.apply_pattern(&p, SR);
        let mut c = Clock::new(SR);
        c.start(&p);
        let mut eb = EventBuf::new();
        let mut out = vec![0.0f32; 512];
        let mut peak = 0.0f32;
        for _ in 0..(8 * SR as usize / 256) {
            eb.clear();
            c.process(&p, 256, &mut eb);
            r.render(eb.as_slice(), &mut out, 256);
            peak = out.iter().fold(peak, |m, x| m.max(x.abs()));
        }
        eb.clear();
        c.stop(&mut eb);
        r.render(eb.as_slice(), &mut out, 256);
        for _ in 0..(8 * SR as usize / 256) {
            r.render(&[], &mut out, 256);
        }
        assert!(peak <= CEIL + 1e-3 && r.overs == 0, "{}: peak {peak}", DEMOS[i].name);
        assert_eq!(r.active_voices(), 0);
        assert!(out.iter().all(|x| x.abs() < 1e-3), "{}: echo tail ends", DEMOS[i].name);
    }
}

#[test]
fn audition_slot_plays_regardless_of_track_mutes_and_stops() {
    let mut r = Rack::new(SR, Bank::builtin());
    r.out_gain = 1.0;
    let mut p = Pattern::empty();
    for t in p.tracks.iter_mut() {
        t.mute = true;
    }
    r.apply_pattern(&p, SR);
    let i = r.bank.find("pad.glass").unwrap();
    assert!(r.set_audition(i, Macros::default()));
    let mut out = vec![0.0f32; 512];
    let on = [Event { offset: 0, kind: EventKind::NoteOn { track: AUDITION as u8, id: 9, pitch: Pitch::from_note(57.0), vel: 0.8 } }];
    r.render(&on, &mut out, 256);
    let mut pk = 0.0f32;
    for _ in 0..40 {
        r.render(&[], &mut out, 256);
        pk = out.iter().fold(pk, |m, x| m.max(x.abs()));
    }
    assert!(pk > 0.02, "audition audible");
    r.render(&[Event { offset: 0, kind: EventKind::AllNotesOff }], &mut out, 256);
    for _ in 0..(4 * SR as usize / 256) {
        r.render(&[], &mut out, 256);
    }
    assert_eq!(r.active_voices(), 0);
}

#[test]
fn solo_via_pattern_silences_other_tracks() {
    let mut p = (DEMOS[0].build)();
    p.tracks[2].solo = true;
    let mut r = Rack::new(SR, Bank::builtin());
    r.apply_pattern(&p, SR);
    assert!(r.slots[0].mute && r.slots[1].mute && !r.slots[2].mute && r.slots[3].mute);
}
