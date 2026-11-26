// Copyright (C) 2026 Paul Lambert
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! USB Lab (Tools > USB Lab): interrogate what is on the USB port in host
//! mode, read-only. List: every device (hubs included) with its kinds;
//! red marker = BadUSB-style indicators. OK = details (descriptors, HID
//! collections and report sizes, endpoints, drivers). Square = save a
//! text + JSON report to /data/zpod/appdata/usb-lab. In gadget mode the app
//! offers the usual "switch USB to devices" dialog (Triangle in host mode
//! gives the port back to the computer).
//! Simulator: `ZPOD_USB_SYSFS=<dir>` points the scan at a fake sysfs tree.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use zpod_core::{Key, KeyState};
use zpod_hal::Platform;
use zpod_input::role::ask::{Outcome, UsbAsk};
use zpod_input::UsbRole;
use zpod_labui::{self as ui, Item};
use zpod_ui::widgets::ListState;
use zpod_usb::known::Known;
use zpod_usb::{dump, scan::scan_at, Device};

static STOP: AtomicBool = AtomicBool::new(false);
extern "C" fn on_sigterm(_: libc::c_int) {
    STOP.store(true, Ordering::SeqCst);
}

fn sysfs() -> PathBuf {
    std::env::var_os("ZPOD_USB_SYSFS").map(PathBuf::from).unwrap_or_else(|| "/sys/bus/usb/devices".into())
}

fn save(devs: &[Device]) -> Result<String, String> {
    let dir = ui::appdata("usb-lab");
    let st = ui::stamp();
    let base = format!("usb-{st}");
    std::fs::write(dir.join(format!("{base}.txt")), dump::text(devs, &st)).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(format!("{base}.json")), dump::json(devs, &st)).map_err(|e| e.to_string())?;
    Ok(format!("Saved {base}"))
}

fn main() {
    let mut p = match Platform::open() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("usb-lab: cannot open display: {e}");
            std::process::exit(1);
        }
    };
    unsafe {
        libc::signal(libc::SIGTERM, on_sigterm as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, on_sigterm as *const () as libc::sighandler_t);
    }
    let sim = std::env::var_os("ZPOD_USB_SYSFS").is_some();
    let mut role = if sim { UsbRole::Host } else { zpod_input::role::usb_role() };
    let known = Known::load();
    let mut devs: Vec<Device> = Vec::new();
    let mut ls = ListState::default();
    let mut detail: Option<(usize, usize)> = None; // (device index, scroll)
    let mut ask = UsbAsk::new("USB Lab");
    let mut note: Option<(String, Instant)> = None;
    let mut next_scan = Instant::now();
    let mut st = p.status.poll();
    let mut dirty = true;
    eprintln!("usb-lab: USB role {}", role.label());
    if role == UsbRole::Gadget {
        if let Err(e) = ask.open(true) {
            eprintln!("usb-lab: cannot ask for USB host: {e}");
        }
    }
    loop {
        if STOP.load(Ordering::Relaxed) {
            break;
        }
        if role != UsbRole::Gadget && Instant::now() >= next_scan {
            let fresh = scan_at(&sysfs(), &known);
            if fresh != devs {
                // keep the selected device across rescans
                let sel_path = devs.get(ls.sel).map(|d| d.path.clone());
                devs = fresh;
                if let Some(sp) = sel_path {
                    if let Some(i) = devs.iter().position(|d| d.path == sp) {
                        ls.sel = i;
                    }
                }
                ui::clamp(&mut ls, devs.len());
                if detail.is_some_and(|(i, _)| i >= devs.len()) {
                    detail = None;
                }
                dirty = true;
            }
            next_scan = Instant::now() + Duration::from_millis(1500);
        }
        match ask.poll() {
            Outcome::Switched(r) => {
                role = r;
                note = Some((if r == UsbRole::Gadget { "Computer link back".into() } else { "USB: devices. Plug something in".into() }, Instant::now()));
                next_scan = Instant::now();
                devs.clear();
                dirty = true;
            }
            Outcome::Failed(e) => {
                note = Some((format!("USB switch failed: {e}"), Instant::now()));
                dirty = true;
            }
            _ => {}
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
            draw(&mut p.fb, &st, role, &devs, &ls, detail);
            ui::draw_ask(&mut p.fb, &ask, note.as_ref().map(|(n, _)| n.as_str()));
            p.flush();
            dirty = false;
        }
        let Some(ev) = p.next_event(Some(Duration::from_millis(100))) else { continue };
        if ev.state == KeyState::Release {
            continue;
        }
        dirty = true;
        if ask.active() {
            if ev.key == Key::Home {
                break;
            }
            if let Some(a) = ui::ask_answer(ev.key) {
                if ask.answer(a) == Outcome::NeedForce {
                    note = Some(("Needs Wi-Fi SSH (or Triangle)".into(), Instant::now()));
                }
            }
            continue;
        }
        if let Some((i, scroll)) = detail.as_mut() {
            let n = devs.get(*i).map(|d| dump::lines(d).len()).unwrap_or(0);
            match ev.key {
                Key::Home => break,
                Key::Left | Key::Cross => detail = None,
                Key::Up => *scroll = scroll.saturating_sub(1),
                Key::Down => *scroll = (*scroll + 1).min(ui::max_scroll(n)),
                Key::Square => note = Some((save(&devs).unwrap_or_else(|e| format!("Save failed: {e}")), Instant::now())),
                _ => {}
            }
            continue;
        }
        match ev.key {
            Key::Home => break,
            Key::Up => ls.step(-1, devs.len(), ui::VISIBLE),
            Key::Down => ls.step(1, devs.len(), ui::VISIBLE),
            Key::Ok | Key::Right | Key::Circle if role == UsbRole::Gadget => {
                if let Err(e) = ask.open(true) {
                    note = Some((format!("Cannot switch here: {e}"), Instant::now()));
                }
            }
            Key::Ok | Key::Right | Key::Circle if !devs.is_empty() => detail = Some((ls.sel, 0)),
            Key::Square if !devs.is_empty() => note = Some((save(&devs).unwrap_or_else(|e| format!("Save failed: {e}")), Instant::now())),
            Key::Triangle if role == UsbRole::Host && !sim => {
                if let Err(e) = ask.open(false) {
                    note = Some((format!("Cannot switch here: {e}"), Instant::now()));
                }
            }
            _ => {}
        }
    }
    eprintln!("usb-lab: exit");
}

fn draw(fb: &mut zpod_ui::Framebuffer, st: &zpod_core::StatusInfo, role: UsbRole, devs: &[Device], ls: &ListState, detail: Option<(usize, usize)>) {
    if role == UsbRole::Gadget {
        ui::header(fb, st, "USB Lab", "computer link", ui::CYAN, "USB is connected to a computer");
        ui::message(fb, "USB host mode needed", &["USB Lab inspects devices plugged into the ZPOD.", "OK: switch USB to devices (Tools > USB also works).", "Use an OTG adapter or a powered hub."]);
        ui::hints(fb, &[("OK", "switch USB"), ("Home", "exit")]);
        return;
    }
    let flagged = devs.iter().filter(|d| !d.indicators().is_empty()).count();
    let info = if flagged > 0 { format!("{} device(s), {flagged} flagged", devs.len()) } else { format!("{} device(s)", devs.len()) };
    if let Some((i, scroll)) = detail {
        let Some(d) = devs.get(i) else { return };
        ui::header(fb, st, "USB Lab", &d.id(), if d.indicators().is_empty() { ui::OK } else { ui::WARN }, &d.title());
        let lines = dump::lines(d);
        ui::detail(fb, &lines, scroll);
        ui::hints(fb, &[("Up/Dn", "scroll"), ("Sq", "save report"), ("Lt", "back"), ("Home", "exit")]);
        return;
    }
    ui::header(fb, st, "USB Lab", "devices", ui::OK, &info);
    if devs.is_empty() {
        ui::message(fb, "Nothing plugged in", &["Plug a device into the OTG port (or a powered hub).", "The list updates by itself.", "Read-only: nothing is sent to the device."]);
        ui::hints(fb, &[("Tri", "computer link"), ("Home", "exit")]);
        return;
    }
    let items: Vec<Item> = devs
        .iter()
        .map(|d| {
            let ind = d.indicators();
            Item {
                title: d.title(),
                sub: if ind.is_empty() { format!("{}  {} Mb/s", d.kinds(), d.speed.as_deref().unwrap_or("?")) } else { format!("! {}", ind[0]) },
                right: d.id(),
                mark: Some(if !ind.is_empty() { ui::WARN } else if d.is_hub() { ui::FAINT } else { ui::OK }),
            }
        })
        .collect();
    ui::list(fb, &items, ls);
    ui::hints(fb, &[("OK", "details"), ("Sq", "save report"), ("Tri", "computer link"), ("Home", "exit")]);
}
