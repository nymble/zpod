//! GPS tool for ZPOD (Tools > GPS): GlobalSat ND-105C and other serial NMEA
//! receivers on the USB port in host mode. Shows the receiver state
//! (unsupported / no device / no data / searching / 2D / 3D / stale /
//! error), coordinates, UTC time, satellites used / in view, HDOP and one
//! SNR bar per satellite.
//!
//! Buttons: OK = decimal degrees / degrees + minutes, Home = exit.
//! Simulator: `ZPOD_GPS_SIM=<nmea fixture>` (see serial::run_sim),
//! `ZPOD_USB_ROLE=gadget|host`.

mod serial;
mod ui;

use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

use zpod_core::{Key, KeyState};
use zpod_hal::Platform;
use zpod_input::UsbRole;
use zpod_nmea::{GpsState, GpsStatus};

use std::sync::atomic::{AtomicBool, Ordering};
static STOP: AtomicBool = AtomicBool::new(false);
extern "C" fn on_sigterm(_: libc::c_int) {
    STOP.store(true, Ordering::SeqCst);
}

pub struct View {
    pub status: GpsStatus,
    pub port: Option<String>,
    pub dmm: bool,
    /// ms since the last fix (for "stale 12 s")
    pub fix_age_ms: Option<u64>,
}

fn main() {
    let mut p = match Platform::open() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("gps: cannot open display: {e}");
            std::process::exit(1);
        }
    };
    unsafe {
        libc::signal(libc::SIGTERM, on_sigterm as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, on_sigterm as *const () as libc::sighandler_t);
    }
    let role = zpod_input::role::usb_role();
    let sim = std::env::var("ZPOD_GPS_SIM").ok();
    let drivers = sim.is_some() || serial::drivers_present();
    let unsupported = match (role, drivers) {
        (UsbRole::Gadget, _) => Some("USB is the computer link (gadget mode)".to_string()),
        (_, false) => Some("This image has no USB serial (pl2303) driver".to_string()),
        _ => None,
    };
    eprintln!("gps: USB role {}, drivers {}", role.label(), if drivers { "present" } else { "missing" });

    let (tx, rx) = channel();
    if unsupported.is_none() {
        match sim {
            Some(path) => {
                std::thread::spawn(move || serial::run_sim(&path, tx));
            }
            None => {
                std::thread::spawn(move || serial::run_reader(tx));
            }
        }
    }

    let epoch = Instant::now();
    let now_ms = || epoch.elapsed().as_millis() as u64;
    let mut gps = GpsState::default();
    let mut port: Option<serial::Port> = None;
    let mut io_error: Option<String> = None;
    let mut dmm = false;
    let mut last_status = None;
    let mut next_redraw = Instant::now();
    let mut dirty = true;
    let mut st = p.status.poll();

    loop {
        if STOP.load(Ordering::Relaxed) {
            break;
        }
        while let Ok(m) = rx.try_recv() {
            match m {
                serial::Msg::NoDevice => {
                    port = None;
                    io_error = None;
                    gps.reset();
                }
                serial::Msg::Opened(pt) => {
                    port = Some(pt);
                    io_error = None;
                    gps.reset();
                }
                serial::Msg::OpenError(pt, e) => {
                    io_error = Some(if e.contains("ermission") { format!("No permission for /dev/{} (needs group dialout)", pt.name) } else { format!("/dev/{}: {e}", pt.name) });
                    port = Some(pt);
                }
                serial::Msg::Line(l) => {
                    let _ = gps.feed(&l, now_ms());
                }
                serial::Msg::Lost(e) => {
                    io_error = Some(e);
                }
            }
            dirty = true;
        }
        let now = now_ms();
        let status = match &unsupported {
            Some(r) => GpsStatus::Unsupported(r.clone()),
            None => gps.status(now, port.is_some(), io_error.as_deref()),
        };
        if last_status.as_ref() != Some(&status) {
            eprintln!("gps: {}", status.label());
            last_status = Some(status.clone());
            dirty = true;
        }
        let s2 = p.status.poll();
        if s2 != st {
            st = s2;
            dirty = true;
        }
        if dirty || Instant::now() >= next_redraw {
            let v = View { status, port: port.as_ref().map(|p| p.describe()), dmm, fix_age_ms: gps.last_fix_ms.map(|t| now.saturating_sub(t)) };
            ui::draw(&mut p.fb, &st, &v, &gps);
            p.flush();
            dirty = false;
            next_redraw = Instant::now() + Duration::from_millis(500);
        }
        let Some(ev) = p.next_event(Some(Duration::from_millis(100))) else { continue };
        if ev.state != KeyState::Press {
            continue;
        }
        match ev.key {
            Key::Home => break,
            Key::Ok | Key::Circle => {
                dmm = !dmm;
                dirty = true;
            }
            _ => {}
        }
    }
    eprintln!("gps: exit");
}
