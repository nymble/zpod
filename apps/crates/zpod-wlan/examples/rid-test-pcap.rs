//! Write an OFFLINE pcap of a labelled, moving TEST Remote ID aircraft.
//! Runs on a computer (no radio). Usage:
//!   cargo run -p zpod-wlan --example rid-test-pcap -- OUT.pcap [lat lon speed_mps track_deg climb_mps turn_dps seconds channel]
use zpod_remoteid::SimAircraft;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let Some(out) = a.first() else {
        eprintln!("usage: rid-test-pcap OUT.pcap [lat lon speed_mps track_deg climb_mps turn_dps seconds channel]");
        std::process::exit(2);
    };
    let f = |i: usize, d: f64| a.get(i).and_then(|s| s.parse().ok()).unwrap_or(d);
    let mut ac = SimAircraft::test(f(1, 37.7749), f(2, -122.4194));
    ac.speed_mps = f(3, 10.0) as f32;
    ac.track_deg = f(4, 90.0) as f32;
    ac.climb_mps = f(5, 0.0) as f32;
    ac.turn_dps = f(6, 0.0) as f32;
    let secs = f(7, 60.0) as u32;
    let ch = f(8, 6.0) as u32;
    let start = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_micros() as u64;
    let file = std::io::BufWriter::new(std::fs::File::create(out).expect("create"));
    let n = zpod_wlan::testgen::write_track_pcap(file, &mut ac, ch, secs, 1.0, start).expect("write");
    println!("{out}: {n} TEST beacons, ch {ch}, end {:.6},{:.6} alt {:.0} m track {:.0}", ac.lat, ac.lon, ac.alt_m, ac.track_deg);
}
