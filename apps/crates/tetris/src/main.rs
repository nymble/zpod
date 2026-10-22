//! ZPOD Tetris frontend. All game rules live in `tetris-core` (no_std); this
//! file only maps ZPOD buttons to actions, drives time, and persists scores.
//!
//! Controls: Left/Right move (hold = auto-shift), Down soft drop, Up hard
//! drop, Circle rotate CW, Triangle/X rotate CCW, Square hold,
//! Play/Pause pause, OK start/rotate, Home quit.

use std::time::{Duration, Instant};

use tetris_core::{render::Renderer, scores::Entry, scores::HighScores, Action, Game, Phase};
use zpod_core::{Event, Key, KeyState, StatusInfo};
use zpod_hal::{FileStorage, Platform};
use zpod_ui::status::draw_status_bar;

fn map(g: &Game, e: Event) -> Option<Action> {
    let press = e.state == KeyState::Press;
    let release = e.state == KeyState::Release;
    let playing = g.phase == Phase::Playing;
    Some(match (e.key, press, release) {
        (Key::Left, true, _) => Action::LeftDown,
        (Key::Left, _, true) => Action::LeftUp,
        (Key::Right, true, _) => Action::RightDown,
        (Key::Right, _, true) => Action::RightUp,
        (Key::Down, true, _) => Action::SoftDown,
        (Key::Down, _, true) => Action::SoftUp,
        (Key::Up, true, _) if playing => Action::HardDrop,
        (Key::Circle, true, _) => Action::RotateCw,
        (Key::Triangle | Key::Cross, true, _) => Action::RotateCcw,
        (Key::Square, true, _) => Action::Hold,
        (Key::PlayPause, true, _) => Action::TogglePause,
        (Key::Ok, true, _) if playing => Action::RotateCw,
        (Key::Ok, true, _) => Action::Start,
        _ => return None,
    })
}

fn main() {
    let mut p = match Platform::open() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("tetris: cannot open display: {e}");
            std::process::exit(1);
        }
    };
    let mut store = FileStorage::for_app("tetris");
    let mut hs = HighScores::load(&mut store);
    let seed = std::env::var("ZPOD_SEED").ok().and_then(|s| s.parse().ok()).unwrap_or_else(|| {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos() ^ d.as_secs() as u32).unwrap_or(1)
    });
    let mut g = Game::new(seed);
    let mut r = Renderer::new();
    let mut status = StatusInfo::default();
    let mut last = Instant::now();
    let mut was_over = false;

    loop {
        // status bar (time, battery, ...): redraw only on change
        let st = p.status.poll();
        if st != status {
            draw_status_bar(&mut p.fb, "Tetris", &st);
            status = st;
        }
        r.draw(&mut p.fb, &g, hs.best());
        p.flush();

        let active = matches!(g.phase, Phase::Playing | Phase::Clearing { .. }) || g.last.show_ms > 0;
        let timeout = if active { Duration::from_millis(20) } else { Duration::from_millis(1000) };
        if let Some(ev) = p.next_event(Some(timeout)) {
            match ev.key {
                Key::Home if ev.state == KeyState::Press => break,
                Key::VolUp | Key::VolDown => p.status.volume_changed(),
                _ => {}
            }
            if let Some(a) = map(&g, ev) {
                g.action(a);
            }
        }
        let now = Instant::now();
        let dt = now.duration_since(last).as_millis().min(100) as u32;
        last = now;
        g.tick(dt);

        let over = g.phase == Phase::GameOver;
        if over && !was_over {
            if hs.submit(Entry { score: g.score, lines: g.lines, level: g.level }).is_some() {
                hs.save(&mut store);
            }
            r.invalidate();
        }
        was_over = over;
    }
    if g.score > 0 && g.phase != Phase::GameOver && hs.submit(Entry { score: g.score, lines: g.lines, level: g.level }).is_some() {
        hs.save(&mut store);
    }
}
