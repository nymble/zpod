//! BLE Lab (Tools > BLE Lab): passive Bluetooth LE advertising scanner on
//! the onboard radio. Receive only: the lab helper (zpod-labd) streams the
//! kernel's HCI monitor channel and runs a *passive* LE scan (no scan
//! requests, no advertising, no connections).
//!
//! List: one row per address, drones (Remote ID) and trackers first; OK =
//! detail with every AD field and raw hex; Square = capture on/off (JSONL
//! of every report to /data/zpod/appdata/ble-lab); Triangle = sort.
//! Simulator: `ZPOD_BLE_REPLAY=<file>` replays hex HCI monitor packets
//! (one per line) instead of the helper.

use std::fs::File;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};

use zpod_ble::table::{jsonl, Sort, Table};
use zpod_ble::{parse_monitor, AdvReport, Class};
use zpod_core::{Key, KeyState};
use zpod_hal::Platform;
use zpod_labui::{self as ui, Item};
use zpod_ui::widgets::ListState;

static STOP: AtomicBool = AtomicBool::new(false);
extern "C" fn on_sigterm(_: libc::c_int) {
    STOP.store(true, Ordering::SeqCst);
}

enum Msg {
    Up(String),
    Down(String),
    Reports(Vec<AdvReport>),
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect()
}

fn reader(tx: Sender<Msg>) {
    if let Some(p) = std::env::var_os("ZPOD_BLE_REPLAY") {
        let _ = tx.send(Msg::Up("replay".into()));
        let text = std::fs::read_to_string(p).unwrap_or_default();
        loop {
            for l in text.lines() {
                if let Some(b) = unhex(l) {
                    if tx.send(Msg::Reports(parse_monitor(&b))).is_err() {
                        return;
                    }
                }
                std::thread::sleep(Duration::from_millis(150));
            }
        }
    }
    loop {
        match zpod_labd::client::Stream::open("hci-stream") {
            Ok(mut s) => {
                if tx.send(Msg::Up("passive scan".into())).is_err() {
                    return;
                }
                loop {
                    match s.next() {
                        Ok(pkt) => {
                            let r = parse_monitor(&pkt);
                            if !r.is_empty() && tx.send(Msg::Reports(r)).is_err() {
                                return;
                            }
                        }
                        Err(e) => {
                            if tx.send(Msg::Down(format!("stream ended: {e}"))).is_err() {
                                return;
                            }
                            break;
                        }
                    }
                }
            }
            Err(e) => {
                if tx.send(Msg::Down(e)).is_err() {
                    return;
                }
            }
        }
        std::thread::sleep(Duration::from_secs(3));
    }
}

struct Capture {
    f: File,
    name: String,
    n: u64,
}

fn main() {
    let mut p = match Platform::open() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("ble-lab: cannot open display: {e}");
            std::process::exit(1);
        }
    };
    unsafe {
        libc::signal(libc::SIGTERM, on_sigterm as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, on_sigterm as *const () as libc::sighandler_t);
    }
    let (tx, rx): (Sender<Msg>, Receiver<Msg>) = channel();
    std::thread::Builder::new().name("ble-reader".into()).spawn(move || reader(tx)).expect("reader thread");
    let epoch = Instant::now();
    let now_ms = || epoch.elapsed().as_millis() as u64;
    let mut table = Table::new(200);
    let mut sort = Sort::Signal;
    let mut ls = ListState::default();
    let mut detail: Option<(String, usize)> = None; // (addr, scroll)
    let mut link: Result<String, String> = Err("starting...".into());
    let mut cap: Option<Capture> = None;
    let mut note: Option<(String, Instant)> = None;
    let mut st = p.status.poll();
    let mut next_redraw = Instant::now();
    let mut dirty = true;
    loop {
        if STOP.load(Ordering::Relaxed) {
            break;
        }
        while let Ok(m) = rx.try_recv() {
            match m {
                Msg::Up(s) => {
                    eprintln!("ble-lab: {s}");
                    link = Ok(s);
                    dirty = true;
                }
                Msg::Down(e) => {
                    if link.as_ref().err() != Some(&e) {
                        eprintln!("ble-lab: {e}");
                    }
                    link = Err(e);
                    dirty = true;
                }
                Msg::Reports(rs) => {
                    let t = now_ms();
                    for r in rs {
                        if let Some(i) = table.feed(&r, t) {
                            if let Some(c) = cap.as_mut() {
                                let _ = c.f.write_all(jsonl(&r, t, &table.rows[i].class.short()).as_bytes());
                                c.n += 1;
                            }
                        }
                    }
                }
            }
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
        if dirty || Instant::now() >= next_redraw {
            table.expire(now_ms(), 120_000);
            let order = table.sorted(sort);
            ui::clamp(&mut ls, order.len());
            draw(&mut p.fb, &st, &table, &order, &ls, &detail, &link, &cap, sort, now_ms());
            if let Some((n, _)) = &note {
                ui::toast(&mut p.fb, n);
            }
            p.flush();
            dirty = false;
            next_redraw = Instant::now() + Duration::from_millis(700);
        }
        let Some(ev) = p.next_event(Some(Duration::from_millis(100))) else { continue };
        if ev.state == KeyState::Release {
            continue;
        }
        dirty = true;
        let order = table.sorted(sort);
        if let Some((addr, scroll)) = detail.as_mut() {
            let n = table.rows.iter().find(|r| &r.addr == addr).map(|r| r.lines(now_ms()).len()).unwrap_or(0);
            match ev.key {
                Key::Home => break,
                Key::Left | Key::Cross => detail = None,
                Key::Up => *scroll = scroll.saturating_sub(1),
                Key::Down => *scroll = (*scroll + 1).min(ui::max_scroll(n)),
                Key::Square => {
                    if let Some(r) = table.rows.iter().find(|r| &r.addr == addr) {
                        let name = format!("ble-{}-{}.txt", r.addr.replace(':', ""), ui::stamp());
                        let res = std::fs::write(ui::appdata("ble-lab").join(&name), r.lines(now_ms()).join("\n") + "\n");
                        note = Some((if res.is_ok() { format!("Saved {name}") } else { "Save failed".into() }, Instant::now()));
                    }
                }
                _ => {}
            }
            continue;
        }
        match ev.key {
            Key::Home => break,
            Key::Up => ls.step(-1, order.len(), ui::VISIBLE),
            Key::Down => ls.step(1, order.len(), ui::VISIBLE),
            Key::Ok | Key::Right | Key::Circle => {
                if let Some(&i) = order.get(ls.sel) {
                    detail = Some((table.rows[i].addr.clone(), 0));
                }
            }
            Key::Triangle => {
                sort = sort.next();
                note = Some((format!("Sort: {}", sort.label()), Instant::now()));
            }
            Key::Square => {
                if let Some(c) = cap.take() {
                    note = Some((format!("Capture saved: {} ({} reports)", c.name, c.n), Instant::now()));
                } else {
                    let name = format!("ble-{}.jsonl", ui::stamp());
                    match File::create(ui::appdata("ble-lab").join(&name)) {
                        Ok(f) => {
                            cap = Some(Capture { f, name, n: 0 });
                            note = Some(("Capture on".into(), Instant::now()));
                        }
                        Err(e) => note = Some((format!("Capture failed: {e}"), Instant::now())),
                    }
                }
            }
            _ => {}
        }
    }
    eprintln!("ble-lab: exit ({} reports)", table.reports);
}

#[allow(clippy::too_many_arguments)]
fn draw(fb: &mut zpod_ui::Framebuffer, st: &zpod_core::StatusInfo, t: &Table, order: &[usize], ls: &ListState, detail: &Option<(String, usize)>, link: &Result<String, String>, cap: &Option<Capture>, sort: Sort, now: u64) {
    let drones = t.rows.iter().filter(|r| matches!(r.class, Class::RemoteId(..))).count();
    let info = format!("{} devices{}  {}{}", t.rows.len(), if drones > 0 { format!(", {drones} drone(s)") } else { String::new() }, sort.label(), if let Some(c) = cap { format!("  REC {}", c.n) } else { String::new() });
    if let Some((addr, scroll)) = detail {
        let Some(r) = t.rows.iter().find(|r| &r.addr == addr) else {
            ui::header(fb, st, "BLE Lab", "gone", ui::FAINT, addr);
            ui::message(fb, "Device expired", &["Not heard for 2 minutes."]);
            ui::hints(fb, &[("Lt", "back")]);
            return;
        };
        ui::header(fb, st, "BLE Lab", &format!("{} dBm", r.rssi), if r.class.notable() { ui::ACCENT } else { ui::OK }, &r.title());
        ui::detail(fb, &r.lines(now), *scroll);
        ui::hints(fb, &[("Up/Dn", "scroll"), ("Sq", "save"), ("Lt", "back"), ("Home", "exit")]);
        return;
    }
    match link {
        Ok(s) => ui::header(fb, st, "BLE Lab", s, if cap.is_some() { ui::WARN } else { ui::OK }, &info),
        Err(_) => ui::header(fb, st, "BLE Lab", "no scan", ui::WARN, &info),
    }
    if t.rows.is_empty() {
        match link {
            Err(e) => ui::message(fb, "Not scanning", &[e, "The lab helper (zpod-labd) runs the passive scan.", "Bluetooth must be on (Tools > Bluetooth)."]),
            Ok(_) => ui::message(fb, "Listening...", &["Passive scan: this ZPOD sends nothing.", "Devices appear as they advertise."]),
        }
        ui::hints(fb, &[("Sq", if cap.is_some() { "stop capture" } else { "capture" }), ("Home", "exit")]);
        return;
    }
    let items: Vec<Item> = order
        .iter()
        .map(|&i| {
            let r = &t.rows[i];
            Item {
                title: r.title(),
                sub: format!("{}  {}  {}", r.class.short(), r.addr, ui::ago(now.saturating_sub(r.last_ms))),
                right: format!("{}", r.rssi),
                mark: Some(if matches!(r.class, Class::RemoteId(..)) { ui::ACCENT } else if r.class.notable() { ui::WARN } else { ui::FAINT }),
            }
        })
        .collect();
    ui::list(fb, &items, ls);
    ui::hints(fb, &[("OK", "detail"), ("Sq", if cap.is_some() { "stop rec" } else { "capture" }), ("Tri", "sort"), ("Home", "exit")]);
}
