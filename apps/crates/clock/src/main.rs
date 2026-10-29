//! ZPOD Clock app (a menu entry; no button is dedicated to it).
//! Face: Up/Down = face, Right/OK/Circle = Clock menu (Alarm, Timer, Face,
//! Seconds, 12/24 hour), Triangle = 12/24 h, Square = seconds on/off,
//! Left/X/Home = back to the launcher, Play/Pause = music (global).
//! Alarm and timer settings go to `<data dir>/clock/alarm.conf`; the launcher
//! rings them even when this app is closed (zpod_hal::alarm, M5).
//! Setting the time lives in Settings > Date & time (the launcher runs as
//! root and writes the DS3231 with `hwclock -w`; this app runs unprivileged).
//! Sleeps until the next second (seconds on, menus, timer) or minute.

mod ui;

use std::time::{Duration, Instant};

use clock_core::{Painter, Settings};
use ui::{Action, Screen, Ui};
use zpod_core::{KeyState, StatusInfo, Storage};
use zpod_hal::{alarm, sys, FileStorage, Platform};
use zpod_ui::status::draw_status_bar;

fn main() {
    let mut p = match Platform::open() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("clock: cannot open display: {e}");
            std::process::exit(1);
        }
    };
    let mut store = FileStorage::for_app("clock"); // creates <data dir>/clock (owned by the app user)
    let mut buf = [0u8; 256];
    let mut s = store.load("clock.conf", &mut buf).map(|n| Settings::parse(std::str::from_utf8(&buf[..n]).unwrap_or(""))).unwrap_or_default();
    let mut a = alarm::load();
    let mut painter = Painter::default();
    let mut status = StatusInfo::default();
    let mut title = String::new();
    let mut ui = Ui::default();
    let mut dirty = true;
    let mut last_tick = Instant::now();

    loop {
        let now_unix = sys::unix_now();
        let st = p.status.poll();
        let t = sys::now_local();
        let new_title = ui.title(&s, &a, now_unix);
        if st != status || new_title != title {
            draw_status_bar(&mut p.fb, &new_title, &st);
            status = st.clone();
            title = new_title;
        }
        if ui.screen == Screen::Face {
            painter.draw(&mut p.fb, &t, &s);
        } else if dirty || matches!(ui.screen, Screen::Timer(..)) {
            ui.draw(&mut p.fb, &s, &a, &t, now_unix, st.volume_pct);
            dirty = false;
        }
        p.flush();

        // sleep to the next visible change
        let ms_to_sec = 1000 - sys::subsec_ms().min(999) + 5;
        let per_second = s.seconds || ui.screen != Screen::Face || a.timer_end > 0 || ui.toast.is_some();
        let timeout = if per_second { ms_to_sec.min(if ui.toast.is_some() { 200 } else { 1000 }) } else { (59 - t.second as u64) * 1000 + ms_to_sec };
        let ev = p.next_event(Some(Duration::from_millis(timeout)));
        let dt = last_tick.elapsed().as_millis() as u32;
        last_tick = Instant::now();
        if let Some((_, left)) = ui.toast.as_mut() {
            *left = left.saturating_sub(dt);
            if *left == 0 {
                ui.toast = None;
                dirty = true;
                painter.invalidate();
            }
        }
        let Some(ev) = ev else { continue };
        if ev.state == KeyState::Release {
            continue;
        }
        let was_face = ui.screen == Screen::Face;
        // the launcher may have snoozed/dismissed/cleared since we loaded
        a = alarm::load();
        match ui.key(ev.key, ev.state, &mut s, &mut a, sys::unix_now()) {
            Action::None => {}
            Action::Exit => break,
            Action::PlayPause => {
                let (st, _) = sys::mpd_toggle();
                p.status.set_play(st);
            }
            Action::Redraw => dirty = true,
            Action::ToFace => painter.invalidate(),
            Action::SaveClock => {
                store.save("clock.conf", s.to_text().as_bytes());
                painter.invalidate();
                dirty = true;
            }
            Action::SaveAlarm => {
                if let Err(e) = alarm::save(&a) {
                    eprintln!("clock: cannot save {}: {e}", alarm::path().display());
                    ui.toast = Some(("Cannot save alarm".into(), 2000));
                }
                dirty = true;
            }
        }
        if was_face && ui.screen != Screen::Face {
            dirty = true;
        }
        if ui.screen == Screen::Face && ui.toast.is_some() {
            // toasts are drawn by the menu screens; on the face just repaint
            ui.toast = None;
            painter.invalidate();
        }
    }
}
