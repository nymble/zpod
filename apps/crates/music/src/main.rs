//! ZPOD music player: `music_core` UI + MPD backend + album art loader.
//!
//! Playback, decoding, the tag database and gapless/resampling are done by
//! MPD (C, ALSA output to the PCM5122). This process is only the UI: it can
//! exit (Home) while music keeps playing, and it sleeps between seconds.

mod art;
mod backend;

use std::time::{Duration, Instant};

use music_core::{view, MusicApp, Outcome};
use zpod_core::{PlayState, StatusInfo};
use zpod_hal::Platform;

fn main() {
    let mut p = match Platform::open() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("music: cannot open display: {e}");
            std::process::exit(1);
        }
    };
    p.status.query_mpd = false; // we know the play state ourselves
    let mut be = backend::Mpd::new();
    let mut app = MusicApp::new();
    app.start(&mut be);

    let (req_tx, res_rx) = art::spawn_loader(view::ART);
    let mut requested: Option<String> = None;
    let mut last_si = StatusInfo::default();
    let mut last = Instant::now();
    let mut next_poll = Instant::now();

    loop {
        if let Some((uri, key)) = app.wanted_art() {
            if requested.as_deref() != Some(key.as_str()) {
                let _ = req_tx.send((uri, key.clone()));
                requested = Some(key);
            }
        }
        while let Ok((key, img)) = res_rx.try_recv() {
            if requested.as_deref() == Some(key.as_str()) {
                app.set_art(key, img);
            }
        }
        if let Some(st) = &app.status {
            p.status.set_play(st.state);
            // volume shown = zpod-buttons' value (not MPD's mixer scale)
        } else {
            p.status.set_play(PlayState::Unknown);
        }
        let si = p.status.poll();
        let changed = si != last_si;
        view::render(&mut app, &mut p.fb, &si, changed);
        last_si = si;
        p.flush();

        let playing = app.status.as_ref().is_some_and(|s| s.state == PlayState::Playing);
        let timeout = if app.toast.is_some() {
            Duration::from_millis(100)
        } else if playing {
            // wake at the next whole second of the track
            let e = app.status.as_ref().map(|s| s.elapsed).unwrap_or(0.0);
            Duration::from_millis(((1.0 - e.fract()) * 1000.0) as u64 + 10)
        } else {
            Duration::from_millis(2000)
        };
        if let Some(ev) = p.next_event(Some(timeout)) {
            if app.handle(ev, &mut be) == Outcome::Exit {
                break;
            }
        }
        let now = Instant::now();
        app.tick(now.duration_since(last).as_millis() as u32);
        last = now;
        if now >= next_poll {
            app.poll(&mut be);
            next_poll = now + if playing { Duration::from_secs(1) } else { Duration::from_secs(3) };
        }
    }
}
