//! Render the built-in demo grooves to a 16-bit stereo WAV and print level
//! statistics:  cargo run --release -p zpod-voices --example render_demo -- out.wav [bars]
use std::io::Write;
use std::time::Instant;
use zpod_seq::demos::DEMOS;
use zpod_seq::{Clock, EventBuf};
use zpod_voices::{Bank, Rack};

const SR: u32 = 44_100;

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| "pipit-demo.wav".into());
    let bars: f32 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(4.0);
    let mut all: Vec<f32> = Vec::new();
    let t0 = Instant::now();
    for d in DEMOS {
        let p = (d.build)();
        let mut r = Rack::new(SR, Bank::builtin());
        r.out_gain = 1.0;
        for (t, tr) in p.tracks.iter().enumerate() {
            r.set_track(t, tr.voice.as_str(), tr.level as f32 / 127.0, tr.mute);
        }
        let mut c = Clock::new(SR);
        c.start(&p);
        let mut eb = EventBuf::new();
        let mut out = vec![0.0f32; 512];
        let frames = (bars * 16.0 * SR as f32 * 60.0 / (p.tempo * 4.0)) as usize;
        let start = all.len();
        for _ in 0..frames / 256 {
            eb.clear();
            c.process(&p, 256, &mut eb);
            r.render(eb.as_slice(), &mut out, 256);
            all.extend_from_slice(&out);
        }
        eb.clear();
        c.stop(&mut eb);
        for i in 0..(SR as usize / 256) {
            r.render(if i == 0 { eb.as_slice() } else { &[] }, &mut out, 256);
            all.extend_from_slice(&out);
        }
        let seg = &all[start..];
        let peak = seg.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        let rms = (seg.iter().map(|x| x * x).sum::<f32>() / seg.len() as f32).sqrt();
        let dc = seg.iter().sum::<f32>() / seg.len() as f32;
        println!("{:12} {:5.1} bpm  peak {:6.2} dBFS  rms {:6.2} dBFS  dc {:+.5}  overs {}  voices_after_stop {}", d.name, p.tempo, 20.0 * peak.log10(), 20.0 * rms.log10(), dc, r.overs, r.active_voices());
    }
    let el = t0.elapsed().as_secs_f32();
    let secs = all.len() as f32 / 2.0 / SR as f32;
    let clipped = all.iter().filter(|x| x.abs() >= 0.999).count();
    println!("total {secs:.1} s audio rendered in {el:.2} s ({:.0}x realtime on this host); samples at full scale: {clipped}", secs / el);
    let mut f = std::fs::File::create(&path).expect("create wav");
    let data = (all.len() * 2) as u32;
    let mut h = Vec::new();
    h.extend_from_slice(b"RIFF");
    h.extend_from_slice(&(36 + data).to_le_bytes());
    h.extend_from_slice(b"WAVEfmt ");
    h.extend_from_slice(&16u32.to_le_bytes());
    h.extend_from_slice(&1u16.to_le_bytes());
    h.extend_from_slice(&2u16.to_le_bytes());
    h.extend_from_slice(&SR.to_le_bytes());
    h.extend_from_slice(&(SR * 4).to_le_bytes());
    h.extend_from_slice(&4u16.to_le_bytes());
    h.extend_from_slice(&16u16.to_le_bytes());
    h.extend_from_slice(b"data");
    h.extend_from_slice(&data.to_le_bytes());
    f.write_all(&h).unwrap();
    let pcm: Vec<u8> = all.iter().flat_map(|x| ((x.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes()).collect();
    f.write_all(&pcm).unwrap();
    println!("wrote {path}");
}
