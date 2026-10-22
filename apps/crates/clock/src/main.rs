//! ZPOD Clock app (a menu entry; no button is dedicated to it).
//! Up/Down or Right: face, OK/Circle: seconds on/off, Triangle: 12/24 h,
//! Left/X/Home: back to the launcher, Play/Pause: music (global).
//! Setting the time lives in Settings > Date & time (the launcher runs as
//! root and writes the DS3231 with `hwclock -w`; this app runs unprivileged).
//! Sleeps until the next second (seconds on) or minute (seconds off).

use std::time::Duration;

use clock_core::{Painter, Settings, FACES};
use zpod_core::{Key, KeyState, StatusInfo, Storage};
use zpod_hal::{sys, FileStorage, Platform};
use zpod_ui::status::draw_status_bar;

fn main() {
    let mut p = match Platform::open() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("clock: cannot open display: {e}");
            std::process::exit(1);
        }
    };
    let mut store = FileStorage::for_app("clock");
    let mut buf = [0u8; 256];
    let mut s = store.load("clock.conf", &mut buf).map(|n| Settings::parse(std::str::from_utf8(&buf[..n]).unwrap_or(""))).unwrap_or_default();
    let mut painter = Painter::default();
    let mut status = StatusInfo::default();
    let mut title_face = None;

    loop {
        let st = p.status.poll();
        if st != status || title_face != Some(s.face) {
            let title = format!("Clock \u{b7} {}", s.face.name());
            draw_status_bar(&mut p.fb, &title, &st);
            status = st;
            title_face = Some(s.face);
        }
        let t = sys::now_local();
        painter.draw(&mut p.fb, &t, &s);
        p.flush();

        // sleep to the next visible change
        let ms_to_sec = 1000 - sys::subsec_ms().min(999) + 5;
        let timeout = if s.seconds {
            ms_to_sec
        } else {
            (59 - t.second as u64) * 1000 + ms_to_sec
        };
        let Some(ev) = p.next_event(Some(Duration::from_millis(timeout))) else { continue };
        if ev.state != KeyState::Press {
            continue;
        }
        let mut changed = true;
        match ev.key {
            Key::Home | Key::Cross | Key::Left => break,
            Key::Right | Key::Down => s.face = FACES[(s.face.index() + 1) % FACES.len()],
            Key::Up => s.face = FACES[(s.face.index() + FACES.len() - 1) % FACES.len()],
            Key::PlayPause => {
                let (st, _) = sys::mpd_toggle();
                p.status.set_play(st);
                changed = false;
            }
            Key::Ok | Key::Circle => s.seconds = !s.seconds,
            Key::Triangle => {
                s.h24 = !s.h24;
                painter.invalidate();
            }
            Key::VolUp | Key::VolDown => changed = false, // overlay: platform layer
            _ => changed = false,
        }
        if changed {
            store.save("clock.conf", s.to_text().as_bytes());
        }
    }
}
