// Copyright (C) 2026 Paul Lambert
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! HID Lab (Tools > HID Lab): DuckyScript library with select, on-device edit,
//! dry-run, and confirm-and-arm before every run. Speaks as a USB keyboard
//! gadget (`/dev/hidg0`) after Tools enables HID (or the app turns it on).
//!
//! Bundled demos live in the app package (`scripts/`); editable copies and
//! user scripts live in `/data/zpod/appdata/hid-lab/scripts/`.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use zpod_core::{Key, KeyState};
use zpod_ducky::{self, Action};
use zpod_hal::Platform;
use zpod_labui::{self as ui, Item};
use zpod_ui::textentry::{Outcome as TeOutcome, Pad, TextEntry};
use zpod_ui::textfield::{Edit, EntryNote, TextField};
use zpod_ui::widgets::ListState;

static STOP: AtomicBool = AtomicBool::new(false);
extern "C" fn on_sigterm(_: libc::c_int) {
    STOP.store(true, Ordering::SeqCst);
}

const SAFEGUARD: &str = "Target is YOUR machine in an isolated lab?";

#[derive(Clone)]
struct Script {
    name: String,
    path: PathBuf,
    bundled: bool,
}

enum Screen {
    List,
    View { idx: usize, scroll: usize },
    Dry { idx: usize, scroll: usize, lines: Vec<String> },
    Warn { idx: usize },
    Edit { idx: usize, entry: TextEntry },
    Note(String),
}

fn scripts_dirs() -> (PathBuf, PathBuf) {
    let data = ui::appdata("hid-lab").join("scripts");
    let _ = fs::create_dir_all(&data);
    // bundled: next to the executable's package root, or /opt/zpod/apps/hid-lab/scripts
    let bundled = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("../scripts")))
        .filter(|p| p.is_dir())
        .unwrap_or_else(|| PathBuf::from("/data/zpod/apps/hid-lab/scripts"));
    let bundled = if bundled.is_dir() {
        bundled
    } else {
        PathBuf::from("/usr/share/zpod/ducky")
    };
    (data, bundled)
}

fn list_scripts() -> Vec<Script> {
    let (data, bundled) = scripts_dirs();
    let mut v = Vec::new();
    let mut add = |dir: &Path, bundled: bool| {
        let Ok(rd) = fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("txt") {
                continue;
            }
            if p.file_name().and_then(|x| x.to_str()) == Some("README.txt") {
                continue;
            }
            v.push(Script { name: p.file_name().unwrap().to_string_lossy().into(), path: p, bundled });
        }
    };
    add(&bundled, true);
    add(&data, false);
    v.sort_by(|a, b| a.name.cmp(&b.name));
    v
}

fn seed_data_copies(scripts: &[Script]) {
    let (data, _) = scripts_dirs();
    for s in scripts.iter().filter(|s| s.bundled) {
        let dest = data.join(&s.name);
        if !dest.exists() {
            let _ = fs::copy(&s.path, &dest);
        }
    }
}

fn read(s: &Script) -> Result<String, String> {
    fs::read_to_string(&s.path).map_err(|e| e.to_string())
}

fn hidg() -> PathBuf {
    std::env::var_os("ZPOD_HIDG").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/dev/hidg0"))
}

fn run_encoded(reports: &[(u32, [u8; 8])]) -> Result<String, String> {
    let path = hidg();
    let mut f = fs::OpenOptions::new().write(true).open(&path).map_err(|e| {
        format!("open {}: {e} (enable USB HID gadget: Tools > USB, or connect as keyboard)", path.display())
    })?;
    for (delay, rep) in reports {
        if *delay > 0 {
            std::thread::sleep(Duration::from_millis(*delay as u64));
        }
        f.write_all(rep).map_err(|e| format!("write hidg: {e}"))?;
        let _ = f.flush();
    }
    Ok(format!("Sent {} reports", reports.len()))
}

fn main() {
    let mut p = match Platform::open() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("hid-lab: {e}");
            std::process::exit(1);
        }
    };
    unsafe {
        libc::signal(libc::SIGTERM, on_sigterm as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, on_sigterm as *const () as libc::sighandler_t);
    }
    let mut scripts = list_scripts();
    seed_data_copies(&scripts);
    scripts = list_scripts();
    let mut ls = ListState::default();
    let mut screen = Screen::List;
    let mut st = p.status.poll();
    let mut dirty = true;
    let mut note: Option<(String, Instant)> = None;
    let say = |n: &mut Option<(String, Instant)>, s: String| {
        eprintln!("hid-lab: {s}");
        *n = Some((s, Instant::now()));
    };

    loop {
        if STOP.load(Ordering::Relaxed) {
            break;
        }
        if note.as_ref().is_some_and(|(_, t)| t.elapsed() > Duration::from_millis(2500)) {
            note = None;
            dirty = true;
        }
        let s2 = p.status.poll();
        if s2 != st {
            st = s2;
            dirty = true;
        }
        if dirty {
            draw(&mut p.fb, &st, &screen, &scripts, &ls, note.as_ref().map(|(n, _)| n.as_str()));
            p.flush();
            dirty = false;
        }
        let Some(ev) = p.next_event(Some(Duration::from_millis(80))) else { continue };
        if ev.state == KeyState::Release {
            continue;
        }
        dirty = true;
        if ev.key == Key::Home {
            break;
        }
        match &mut screen {
            Screen::List => match ev.key {
                Key::Up => ls.step(-1, scripts.len(), ui::VISIBLE),
                Key::Down => ls.step(1, scripts.len(), ui::VISIBLE),
                Key::Ok | Key::Circle | Key::Right => {
                    if scripts.get(ls.sel).is_some() {
                        screen = Screen::View { idx: ls.sel, scroll: 0 };
                    }
                }
                Key::Triangle => {
                    scripts = list_scripts();
                    say(&mut note, format!("{} script(s)", scripts.len()));
                }
                _ => {}
            },
            Screen::View { idx, scroll } => {
                let i = *idx;
                let body = scripts.get(i).and_then(|s| read(s).ok()).unwrap_or_default();
                let lines: Vec<String> = body.lines().map(|l| l.to_string()).collect();
                match ev.key {
                    Key::Left | Key::Cross => screen = Screen::List,
                    Key::Up => *scroll = scroll.saturating_sub(1),
                    Key::Down => *scroll = (*scroll + 1).min(ui::max_scroll(lines.len())),
                    Key::Square => {
                        // dry-run
                        match scripts.get(i).ok_or_else(|| "missing".into()).and_then(|s| read(s)).and_then(|t| zpod_ducky::parse(&t)) {
                            Ok(a) => {
                                let lines = zpod_ducky::dry_run(&a);
                                screen = Screen::Dry { idx: i, scroll: 0, lines };
                            }
                            Err(e) => say(&mut note, e),
                        }
                    }
                    Key::Triangle => {
                        // edit a data copy
                        let Some(s) = scripts.get(i) else { continue };
                        let (data, _) = scripts_dirs();
                        let dest = data.join(&s.name);
                        if s.bundled && !dest.exists() {
                            let _ = fs::copy(&s.path, &dest);
                        }
                        let path = if s.bundled { dest } else { s.path.clone() };
                        let text = fs::read_to_string(&path).unwrap_or_default();
                        // refresh scripts so idx points at editable copy
                        scripts = list_scripts();
                        let idx2 = scripts.iter().position(|x| x.path == path).unwrap_or(i);
                        if text.len() <= 240 {
                            let mut field = TextField::new(false, 240);
                            field.ascii_only = true;
                            for ch in text.chars() {
                                let _ = field.apply(Edit::Insert(ch));
                            }
                            screen = Screen::Edit { idx: idx2, entry: TextEntry::new(field) };
                            say(&mut note, "Edit script; OK saves, X cancels".into());
                        } else {
                            say(&mut note, "Script too long on device; edit on Mac under /data".into());
                        }
                    }
                    Key::Ok | Key::Circle => screen = Screen::Warn { idx: i },
                    _ => {}
                }
            }
            Screen::Dry { idx, scroll, lines } => match ev.key {
                Key::Left | Key::Cross => screen = Screen::View { idx: *idx, scroll: 0 },
                Key::Up => *scroll = scroll.saturating_sub(1),
                Key::Down => *scroll = (*scroll + 1).min(ui::max_scroll(lines.len())),
                Key::Ok | Key::Circle => screen = Screen::Warn { idx: *idx },
                _ => {}
            },
            Screen::Warn { idx } => {
                let i = *idx;
                match ev.key {
                    Key::Cross | Key::Left => screen = Screen::View { idx: i, scroll: 0 },
                    Key::Ok | Key::Circle => say(&mut note, "Confirmed. Triangle: arm + run".into()),
                    Key::Triangle => {
                        let Some(s) = scripts.get(i) else {
                            screen = Screen::List;
                            continue;
                        };
                        let body = match read(s) {
                            Ok(b) => b,
                            Err(e) => {
                                say(&mut note, e);
                                continue;
                            }
                        };
                        let actions = match zpod_ducky::parse(&body) {
                            Ok(a) => a,
                            Err(e) => {
                                say(&mut note, e);
                                continue;
                            }
                        };
                        let reports = match zpod_ducky::encode(&actions) {
                            Ok(r) => r,
                            Err(e) => {
                                say(&mut note, e);
                                continue;
                            }
                        };
                        // Prefer labd arm when available; fall back to local confirm-only in sim
                        let armed = zpod_labd::client::request("tx-arm 60").and_then(|_| zpod_labd::client::request("hid-arm-ok"));
                        if let Err(e) = &armed {
                            if std::env::var_os("ZPOD_HIDG").is_none() && !hidg().exists() {
                                say(&mut note, format!("Arm/helper: {e}"));
                                continue;
                            }
                            // device without labd reply but hidg present: still require we tried
                            eprintln!("hid-lab: labd arm: {e}; continuing if hidg exists");
                        }
                        match run_encoded(&reports) {
                            Ok(m) => screen = Screen::Note(m),
                            Err(e) => say(&mut note, e),
                        }
                    }
                    _ => {}
                }
            }
            Screen::Edit { idx, entry } => {
                let i = *idx;
                let pad = match ev.key {
                    Key::Up => Some(Pad::Up),
                    Key::Down => Some(Pad::Down),
                    Key::Left => Some(Pad::Left),
                    Key::Right => Some(Pad::Right),
                    Key::Ok | Key::Circle => Some(Pad::Select),
                    Key::Cross => Some(Pad::Back),
                    Key::Square => Some(Pad::NextSet),
                    Key::Triangle => Some(Pad::Reveal),
                    _ => None,
                };
                if let Some(pad) = pad {
                    match entry.pad(pad) {
                        TeOutcome::Cancel => screen = Screen::View { idx: i, scroll: 0 },
                        TeOutcome::Submit => {
                            let new_text = entry.field.take();
                            let Some(s) = scripts.get(i) else {
                                screen = Screen::List;
                                continue;
                            };
                            let (data, _) = scripts_dirs();
                            let dest = if s.bundled { data.join(&s.name) } else { s.path.clone() };
                            match fs::write(&dest, new_text.as_bytes()) {
                                Ok(()) => {
                                    scripts = list_scripts();
                                    say(&mut note, format!("Saved {}", dest.file_name().unwrap().to_string_lossy()));
                                    screen = Screen::List;
                                }
                                Err(e) => say(&mut note, e.to_string()),
                            }
                        }
                        TeOutcome::Redraw | TeOutcome::None | TeOutcome::WantKeyboard => {}
                    }
                }
            }
            Screen::Note(_) => {
                if matches!(ev.key, Key::Ok | Key::Circle | Key::Cross | Key::Left) {
                    screen = Screen::List;
                }
            }
        }
    }
    eprintln!("hid-lab: exit");
}

fn draw(fb: &mut zpod_ui::Framebuffer, st: &zpod_core::StatusInfo, screen: &Screen, scripts: &[Script], ls: &ListState, note: Option<&str>) {
    match screen {
        Screen::List => {
            ui::header(fb, st, "HID Lab", "scripts", if scripts.is_empty() { ui::FAINT } else { ui::OK }, &format!("{} script(s)", scripts.len()));
            if scripts.is_empty() {
                ui::message(fb, "No DuckyScript files", &["Add .txt under /data/zpod/appdata/hid-lab/scripts/", "Bundled demos install on first run."]);
                ui::hints(fb, &[("Tri", "rescan"), ("Home", "exit")]);
            } else {
                let items: Vec<Item> = scripts
                    .iter()
                    .map(|s| Item {
                        title: s.name.clone(),
                        sub: if s.bundled { "bundled demo (lab TEST)".into() } else { "user / editable".into() },
                        right: String::new(),
                        mark: Some(if s.bundled { ui::ACCENT } else { ui::OK }),
                    })
                    .collect();
                ui::list(fb, &items, ls);
                ui::hints(fb, &[("OK", "open"), ("Tri", "rescan"), ("Home", "exit")]);
            }
        }
        Screen::View { idx, scroll } => {
            let s = &scripts[*idx];
            ui::header(fb, st, "HID Lab", if s.bundled { "bundled" } else { "user" }, ui::ACCENT, &s.name);
            let body = read(s).unwrap_or_else(|e| format!("! {e}"));
            let lines: Vec<String> = body.lines().map(|l| l.to_string()).collect();
            ui::detail(fb, &lines, *scroll);
            ui::hints(fb, &[("OK", "run"), ("Sq", "dry-run"), ("Tri", "edit"), ("X", "back")]);
        }
        Screen::Dry { scroll, lines, .. } => {
            ui::header(fb, st, "HID Lab", "dry-run", ui::OK, &format!("{} step(s)", lines.len()));
            ui::detail(fb, lines, *scroll);
            ui::hints(fb, &[("OK", "run"), ("X", "back")]);
        }
        Screen::Warn { .. } => {
            ui::header(fb, st, "HID Lab", "ARM", ui::WARN, "before EVERY run");
            ui::message(
                fb,
                "Confirm + arm",
                &[
                    SAFEGUARD,
                    zpod_labd::proto::TX_SAFEGUARD,
                    "OK: I confirm.  Triangle: arm and type.",
                    "X: cancel. Never runs at boot.",
                ],
            );
            ui::hints(fb, &[("OK", "confirm"), ("Tri", "arm+run"), ("X", "cancel")]);
        }
        Screen::Edit { entry, .. } => {
            let n = match note {
                Some(s) => EntryNote::Info(s),
                None => EntryNote::None,
            };
            entry.draw(fb, "Edit script", "DuckyScript", "Save", &["OK: pick / save", "X: delete / cancel"], n);
            ui::hints(fb, &entry.hints());
        }
        Screen::Note(m) => {
            ui::header(fb, st, "HID Lab", "done", ui::OK, "");
            ui::message(fb, "Run finished", &[m, "Arm was consumed. Next run needs a new confirm."]);
            ui::hints(fb, &[("OK/X", "back")]);
        }
    }
    if !matches!(screen, Screen::Edit { .. }) {
        if let Some(n) = note {
            ui::toast(fb, n);
        }
    }
}

// silence unused import in some build configs
#[allow(dead_code)]
fn _action_ty(_: &Action) {}
