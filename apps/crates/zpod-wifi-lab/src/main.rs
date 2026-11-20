//! Wi-Fi Lab (Tools > Radio > Wi-Fi Lab): drone Remote ID (ASTM F3411 Wi-Fi
//! Beacon and NAN) with a second, monitor-capable USB Wi-Fi adapter.
//!
//! Receive: Adapters -> OK listens. Aircraft list, channel, sweep, pcap.
//! Transmit (gated): Adapters -> Square opens the safeguard warning; OK
//! confirms lab isolation; Triangle arms and runs a moving TEST aircraft
//! beacon train via zpod-labd (`tx-arm` then `wlan-tx-rid`). Arm is consumed
//! every time; nothing transmits at boot. The onboard radio is never used.
//!
//! Simulator: `ZPOD_WLAN_REPLAY=<pcap>` replays a capture (no helper).

use std::fs::File;
use std::io::BufWriter;
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};

use zpod_core::{Key, KeyState};
use zpod_hal::Platform;
use zpod_input::role::ask::{Outcome, UsbAsk};
use zpod_input::UsbRole;
use zpod_labui::{self as ui, Item};
use zpod_remoteid::{display::aircraft_lines, Tracker};
use zpod_ui::widgets::ListState;
use zpod_wlan::inventory::{freq_to_channel, parse_phy_info};
use zpod_wlan::pcap;
use zpod_wlan::rid::{decode_frame, Stats};
use zpod_wlan::Adapter;

static STOP: AtomicBool = AtomicBool::new(false);
extern "C" fn on_sigterm(_: libc::c_int) {
    STOP.store(true, Ordering::SeqCst);
}

/// Discovery sweep: the 2.4 GHz Remote ID channels, then (dual-band adapter
/// only) a short 5 GHz pass over the common NAN/beacon channels.
const HOP: [u32; 3] = [1, 6, 11];
const HOP5: [u32; 2] = [44, 149];

fn hop_plan(has5: bool) -> Vec<u32> {
    let mut v = HOP.to_vec();
    if has5 {
        v.extend_from_slice(&HOP5);
    }
    v
}

enum Msg {
    Frame(u64, Vec<u8>),
    Ended(String),
}

/// Transmit UI: warn -> confirm -> arm/run.
enum TxPhase {
    Warn { iface: String, has5: bool },
    Running { iface: String, chan: u32, note: String },
    Done { note: String },
}

struct Cand {
    a: Adapter,
    monitor: Option<bool>,
    has5: bool,
}

fn caps(a: &Adapter) -> (Option<bool>, bool) {
    if a.onboard {
        return (Some(false), false);
    }
    match std::process::Command::new("iw").args(["phy", &a.phy, "info"]).output() {
        Ok(o) if o.status.success() => {
            let c = parse_phy_info(&String::from_utf8_lossy(&o.stdout));
            (Some(c.monitor()), c.has_5ghz())
        }
        _ => (None, false),
    }
}

fn now_us() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_micros() as u64).unwrap_or(0)
}

fn start_stream(iface: String, tx: Sender<Msg>) -> Result<Option<UnixStream>, String> {
    if let Some(p) = std::env::var_os("ZPOD_WLAN_REPLAY") {
        let bytes = std::fs::read(p).map_err(|e| e.to_string())?;
        let (_, pk) = pcap::read_all(&bytes[..]).map_err(|e| e.to_string())?;
        std::thread::spawn(move || loop {
            for (_, b) in &pk {
                if tx.send(Msg::Frame(now_us(), b.clone())).is_err() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(400));
            }
        });
        return Ok(None);
    }
    let mut s = zpod_labd::client::Stream::open(&format!("wlan-stream {iface}"))?;
    let closer = s.closer().map_err(|e| e.to_string())?;
    std::thread::Builder::new()
        .name("wlan-reader".into())
        .spawn(move || loop {
            match s.next() {
                Ok(b) => {
                    if tx.send(Msg::Frame(now_us(), b)).is_err() {
                        return;
                    }
                }
                Err(e) => {
                    let _ = tx.send(Msg::Ended(e.to_string()));
                    return;
                }
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(Some(closer))
}

struct Listen {
    iface: String,
    chan: u32,
    hop: bool,
    plan: Vec<u32>,
    hop_at: Instant,
    closer: Option<UnixStream>,
    rx: Receiver<Msg>,
    stats: Stats,
    tracker: Tracker,
    pcap: Option<(pcap::Writer<BufWriter<File>>, String)>,
    ended: Option<String>,
    ls: ListState,
    detail: Option<(String, usize)>,
}

impl Listen {
    fn stop(&mut self) {
        if let Some(c) = self.closer.take() {
            let _ = c.shutdown(Shutdown::Both);
        }
        if let Some((mut w, _)) = self.pcap.take() {
            let _ = w.flush();
        }
    }
}

fn main() {
    let mut p = match Platform::open() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("wifi-lab: cannot open display: {e}");
            std::process::exit(1);
        }
    };
    unsafe {
        libc::signal(libc::SIGTERM, on_sigterm as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, on_sigterm as *const () as libc::sighandler_t);
    }
    let replay = std::env::var_os("ZPOD_WLAN_REPLAY").is_some();
    let mut role = if replay { UsbRole::Host } else { zpod_input::role::usb_role() };
    let mut ask = UsbAsk::new("Wi-Fi Lab");
    let mut cands: Vec<Cand> = Vec::new();
    let mut als = ListState::default();
    let mut listen: Option<Listen> = None;
    let mut tx: Option<TxPhase> = None;
    let mut note: Option<(String, Instant)> = None;
    let mut next_inv = Instant::now();
    let mut st = p.status.poll();
    let mut next_redraw = Instant::now();
    let mut dirty = true;
    let epoch = Instant::now();
    let now_ms = || epoch.elapsed().as_millis() as u64;
    let say = |n: &mut Option<(String, Instant)>, s: String| {
        eprintln!("wifi-lab: {s}");
        *n = Some((s, Instant::now()));
    };
    loop {
        if STOP.load(Ordering::Relaxed) {
            break;
        }
        if listen.is_none() && Instant::now() >= next_inv {
            let inv = if replay { vec![Adapter { iface: "replay0".into(), phy: "phy9".into(), driver: "pcap".into(), bus: "usb".into(), ..Default::default() }] } else { zpod_wlan::inventory() };
            if inv.len() != cands.len() || inv.iter().zip(&cands).any(|(a, c)| a.iface != c.a.iface || a.arphrd != c.a.arphrd) {
                cands = inv
                    .into_iter()
                    .map(|a| {
                        let (m, h) = if replay { (Some(true), false) } else { caps(&a) };
                        Cand { a, monitor: m, has5: h }
                    })
                    .collect();
                // select the first usable adapter
                if let Some(i) = cands.iter().position(|c| !c.a.onboard && c.monitor != Some(false)) {
                    als.sel = i;
                }
                dirty = true;
            }
            next_inv = Instant::now() + Duration::from_secs(2);
        }
        match ask.poll() {
            Outcome::Switched(r) => {
                role = r;
                say(&mut note, if r == UsbRole::Gadget { "Computer link back".into() } else { "USB: devices. Plug in the adapter".into() });
                dirty = true;
            }
            Outcome::Failed(e) => {
                say(&mut note, format!("USB switch failed: {e}"));
                dirty = true;
            }
            _ => {}
        }
        if let Some(l) = listen.as_mut() {
            let mut n = 0;
            while let Ok(m) = l.rx.try_recv() {
                match m {
                    Msg::Frame(ts, b) => {
                        if let Some((w, _)) = l.pcap.as_mut() {
                            let _ = w.packet(ts, &b);
                        }
                        if let Some(d) = decode_frame(&b, &mut l.stats) {
                            let ch = d.freq.map(|f| freq_to_channel(f as u32)).or(Some(l.chan));
                            let i = l.tracker.feed(&d.src, d.transport, d.counter, &d.msgs, d.rssi, ch, now_ms());
                            if l.tracker.aircraft[i].frames == 1 {
                                eprintln!("wifi-lab: aircraft {} via {}", l.tracker.aircraft[i].label(), d.transport.label());
                            }
                        }
                    }
                    Msg::Ended(e) => {
                        l.ended = Some(e);
                        dirty = true;
                    }
                }
                n += 1;
                if n > 500 {
                    break;
                }
            }
            if l.hop && l.hop_at.elapsed() > Duration::from_millis(1500) && !replay {
                let k = l.plan.iter().position(|c| *c == l.chan).map(|k| (k + 1) % l.plan.len()).unwrap_or(0);
                l.chan = l.plan[k];
                let _ = zpod_labd::client::request(&format!("wlan-chan {} {}", l.iface, l.chan));
                l.hop_at = Instant::now();
            }
            l.tracker.expire(now_ms());
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
            if let Some(tp) = tx.as_ref() {
                draw_tx(&mut p.fb, &st, tp);
            } else {
                match listen.as_mut() {
                    Some(l) => draw_listen(&mut p.fb, &st, l, now_ms()),
                    None => draw_adapters(&mut p.fb, &st, role, &cands, &als),
                }
            }
            ui::draw_ask(&mut p.fb, &ask, note.as_ref().map(|(n, _)| n.as_str()));
            p.flush();
            dirty = false;
            next_redraw = Instant::now() + Duration::from_millis(600);
        }
        let Some(ev) = p.next_event(Some(Duration::from_millis(80))) else { continue };
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
                    say(&mut note, "Needs Wi-Fi SSH (or Triangle)".into());
                }
            }
            continue;
        }
        if ev.key == Key::Home {
            break;
        }
        if let Some(tp) = tx.as_mut() {
            match tp {
                TxPhase::Warn { iface, has5: _ } => match ev.key {
                    Key::Cross | Key::Left => {
                        tx = None;
                        say(&mut note, "Transmit cancelled".into());
                    }
                    Key::Ok | Key::Circle => {
                        // confirmed warning; wait for Triangle to arm
                        say(&mut note, "Confirmed. Triangle: arm + transmit 15s TEST".into());
                    }
                    Key::Triangle => {
                        let iface = iface.clone();
                        let chan = 6u32;
                        if replay {
                            tx = Some(TxPhase::Done { note: "Simulator: no RF transmit".into() });
                            continue;
                        }
                        // ensure monitor mode
                        if let Err(e) = zpod_labd::client::request(&format!("wlan-monitor {iface} {chan}")) {
                            say(&mut note, e);
                            tx = None;
                            continue;
                        }
                        if let Err(e) = zpod_labd::client::request("tx-arm 30") {
                            say(&mut note, e);
                            let _ = zpod_labd::client::request(&format!("wlan-managed {iface}"));
                            tx = None;
                            continue;
                        }
                        tx = Some(TxPhase::Running { iface: iface.clone(), chan, note: "Transmitting TEST RID…".into() });
                        draw_tx(&mut p.fb, &st, tx.as_ref().unwrap());
                        p.flush();
                        // blocking TX (up to ~15s + margin)
                        let res = zpod_labd::client::request_timeout(&format!("wlan-tx-rid {iface} {chan} 15"), Duration::from_secs(90));
                        let _ = zpod_labd::client::request(&format!("wlan-managed {iface}"));
                        tx = Some(TxPhase::Done {
                            note: match res {
                                Ok(m) => m,
                                Err(e) => format!("TX failed: {e}"),
                            },
                        });
                    }
                    _ => {}
                },
                TxPhase::Running { .. } => {}
                TxPhase::Done { .. } => {
                    if matches!(ev.key, Key::Ok | Key::Circle | Key::Cross | Key::Left) {
                        tx = None;
                    }
                }
            }
            continue;
        }
        if let Some(l) = listen.as_mut() {
            if let Some((mac, scroll)) = l.detail.as_mut() {
                let n = l.tracker.aircraft.iter().find(|a| &a.mac == mac).map(|a| aircraft_lines(a, 0).len()).unwrap_or(0);
                match ev.key {
                    Key::Left | Key::Cross => l.detail = None,
                    Key::Up => *scroll = scroll.saturating_sub(1),
                    Key::Down => *scroll = (*scroll + 1).min(ui::max_scroll(n)),
                    _ => {}
                }
                continue;
            }
            match ev.key {
                Key::Up => l.ls.step(-1, l.tracker.aircraft.len(), ui::VISIBLE),
                Key::Down => l.ls.step(1, l.tracker.aircraft.len(), ui::VISIBLE),
                Key::Ok | Key::Circle => {
                    if let Some(a) = l.tracker.aircraft.get(l.ls.sel) {
                        l.detail = Some((a.mac.clone(), 0));
                    }
                }
                Key::Left | Key::Right if !replay => {
                    let max = 13;
                    l.hop = false;
                    l.chan = if ev.key == Key::Right { l.chan % max + 1 } else if l.chan <= 1 { max } else { l.chan - 1 };
                    if let Err(e) = zpod_labd::client::request(&format!("wlan-chan {} {}", l.iface, l.chan)) {
                        say(&mut note, format!("Channel: {e}"));
                    }
                }
                Key::Triangle => {
                    l.hop = !l.hop;
                    say(&mut note, if l.hop { format!("Sweeping {}", l.plan.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(" / ")) } else { format!("Fixed on channel {}", l.chan) });
                }
                Key::Square => {
                    if let Some((mut w, name)) = l.pcap.take() {
                        let _ = w.flush();
                        say(&mut note, format!("Saved {name} ({} frames)", w.packets));
                    } else {
                        let name = format!("wifi-{}-ch{}.pcap", ui::stamp(), l.chan);
                        match File::create(ui::appdata("wifi-lab").join(&name)).map_err(|e| e.to_string()).and_then(|f| pcap::Writer::new(BufWriter::new(f), pcap::LINKTYPE_RADIOTAP).map_err(|e| e.to_string())) {
                            Ok(w) => {
                                l.pcap = Some((w, name));
                                say(&mut note, "Recording pcap".into());
                            }
                            Err(e) => say(&mut note, format!("Record failed: {e}")),
                        }
                    }
                }
                Key::Cross => {
                    l.stop();
                    listen = None;
                    next_inv = Instant::now();
                    say(&mut note, "Stopped; adapter back to managed".into());
                }
                _ => {}
            }
            continue;
        }
        // adapters screen
        match ev.key {
            Key::Up => als.step(-1, cands.len(), ui::VISIBLE),
            Key::Down => als.step(1, cands.len(), ui::VISIBLE),
            Key::Ok | Key::Circle | Key::Right if role == UsbRole::Gadget => {
                if let Err(e) = ask.open(true) {
                    say(&mut note, format!("Cannot switch here: {e}"));
                }
            }
            Key::Ok | Key::Circle | Key::Right => {
                let Some(c) = cands.get(als.sel) else { continue };
                if c.a.onboard {
                    say(&mut note, "Onboard Wi-Fi stays on your network".into());
                    continue;
                }
                if c.monitor == Some(false) {
                    say(&mut note, format!("{} ({}) has no monitor mode", c.a.iface, c.a.driver));
                    continue;
                }
                let iface = c.a.iface.clone();
                let plan = hop_plan(c.has5);
                let chan = 6;
                if !replay {
                    if let Err(e) = zpod_labd::client::request(&format!("wlan-monitor {iface} {chan}")) {
                        say(&mut note, e);
                        continue;
                    }
                }
                let (tx, rx) = channel();
                match start_stream(iface.clone(), tx) {
                    Ok(closer) => {
                        eprintln!("wifi-lab: listening on {iface} ch {chan}");
                        listen = Some(Listen { iface, chan, hop: false, plan, hop_at: Instant::now(), closer, rx, stats: Stats::default(), tracker: Tracker::new(180_000), pcap: None, ended: None, ls: ListState::default(), detail: None });
                    }
                    Err(e) => {
                        let _ = zpod_labd::client::request(&format!("wlan-managed {iface}"));
                        say(&mut note, e);
                    }
                }
            }
            Key::Triangle if role == UsbRole::Host && !replay => {
                if let Err(e) = ask.open(false) {
                    say(&mut note, format!("Cannot switch here: {e}"));
                }
            }
            Key::Square => {
                let Some(c) = cands.get(als.sel) else { continue };
                if c.a.onboard {
                    say(&mut note, "Onboard Wi-Fi stays on your network (no TX)".into());
                    continue;
                }
                if c.monitor == Some(false) {
                    say(&mut note, format!("{} has no monitor/inject", c.a.iface));
                    continue;
                }
                tx = Some(TxPhase::Warn { iface: c.a.iface.clone(), has5: c.has5 });
            }
            _ => {}
        }
    }
    if let Some(mut l) = listen.take() {
        l.stop();
    }
    eprintln!("wifi-lab: exit");
}

fn draw_adapters(fb: &mut zpod_ui::Framebuffer, st: &zpod_core::StatusInfo, role: UsbRole, cands: &[Cand], ls: &ListState) {
    let usable = cands.iter().filter(|c| !c.a.onboard && c.monitor != Some(false)).count();
    ui::header(fb, st, "Wi-Fi Lab", "adapters", if usable > 0 { ui::OK } else { ui::FAINT }, &format!("{} radio(s), {usable} usable for monitor", cands.len()));
    if usable == 0 {
        if role == UsbRole::Gadget {
            ui::message(fb, "Plug in a USB Wi-Fi adapter", &["Monitor needs a second, USB adapter (RT5370,", "MT7601U...) via a powered hub. USB is now the", "computer link. OK: switch USB to devices.", "SSH stays up over Wi-Fi; or use a USB hub."]);
            ui::hints(fb, &[("OK", "switch USB"), ("Home", "exit")]);
        } else {
            ui::message(fb, "Plug in a USB Wi-Fi adapter", &["Monitor-capable, via a powered hub:", "RT5370, RT3070 or MT7601U.", "Onboard Wi-Fi stays on your network,", "so SSH still works. Receive only."]);
            ui::hints(fb, &[("Tri", "computer link"), ("Home", "exit")]);
        }
        return;
    }
    let items: Vec<Item> = cands
        .iter()
        .map(|c| {
            let (sub, mark) = if c.a.onboard {
                (format!("{} onboard: stays on your network", c.a.driver), ui::FAINT)
            } else {
                let m = match c.monitor {
                    Some(true) => "monitor ok",
                    Some(false) => "no monitor mode",
                    None => "monitor unknown",
                };
                (format!("{} {}  {}{}", c.a.driver, c.a.usb_id.clone().unwrap_or_default(), m, if c.has5 { ", 5 GHz" } else { "" }), if c.monitor == Some(false) { ui::WARN } else { ui::OK })
            };
            Item { title: format!("{} ({})", c.a.iface, c.a.phy), sub, right: if c.a.in_monitor() { "monitor".into() } else { String::new() }, mark: Some(mark) }
        })
        .collect();
    ui::list(fb, &items, ls);
    ui::hints(fb, &[("OK", "listen"), ("Sq", "TX"), ("Tri", "computer link"), ("Home", "exit")]);
}

fn draw_listen(fb: &mut zpod_ui::Framebuffer, st: &zpod_core::StatusInfo, l: &mut Listen, now: u64) {
    let s = &l.stats;
    let ch = if l.hop { format!("hop {}", l.chan) } else { format!("ch {}", l.chan) };
    let info = format!("{}  {} fr  {} bcn  {} RID{}", l.iface, s.frames, s.beacons, s.rid, if l.pcap.is_some() { "  REC" } else { "" });
    if let Some((mac, scroll)) = &l.detail {
        match l.tracker.aircraft.iter().find(|a| &a.mac == mac) {
            Some(a) => {
                ui::header(fb, st, "Wi-Fi Lab", &ch, if a.emergency() { ui::WARN } else { ui::ACCENT }, &a.label());
                ui::detail(fb, &aircraft_lines(a, now), *scroll);
            }
            None => {
                ui::header(fb, st, "Wi-Fi Lab", &ch, ui::FAINT, mac);
                ui::message(fb, "Aircraft expired", &["Not heard for 3 minutes."]);
            }
        }
        ui::hints(fb, &[("Up/Dn", "scroll"), ("Lt", "back"), ("Home", "exit")]);
        return;
    }
    let pill_bg = if l.ended.is_some() { ui::WARN } else if l.pcap.is_some() { ui::WARN } else { ui::OK };
    ui::header(fb, st, "Wi-Fi Lab", &ch, pill_bg, &info);
    if let Some(e) = &l.ended {
        ui::message(fb, "Capture stopped", &[e, "X: back to adapters (adapter is back to managed)."]);
        ui::hints(fb, &[("X", "back"), ("Home", "exit")]);
        return;
    }
    if l.tracker.aircraft.is_empty() {
        ui::message(fb, "Listening for Remote ID", &["Wi-Fi Beacon and NAN (ASTM F3411).", if l.plan.len() > 3 { "Most drones: ch 6. Tri sweeps 1/6/11/44/149." } else { "Most drones: channel 6. Tri hops 1/6/11." }, "Receive only: nothing is sent."]);
        ui::hints(fb, &[("Lt/Rt", "channel"), ("Tri", "hop"), ("Sq", if l.pcap.is_some() { "stop rec" } else { "record" }), ("X", "stop")]);
        return;
    }
    ui::clamp(&mut l.ls, l.tracker.aircraft.len());
    let items: Vec<Item> = l
        .tracker
        .aircraft
        .iter()
        .map(|a| {
            let pos = a.location.as_ref().and_then(|x| x.lat.zip(x.lon)).map(|(la, lo)| format!("{la:.5},{lo:.5}")).unwrap_or_else(|| "no position".into());
            let alt = a.location.as_ref().and_then(|x| x.height.or(x.alt_geo)).map(|h| format!(" {h:.0}m")).unwrap_or_default();
            Item {
                title: a.label(),
                sub: format!("{}{}  {}  {}", pos, alt, a.transport.map(|t| t.label()).unwrap_or(""), ui::ago(now.saturating_sub(a.last_ms))),
                right: a.rssi.map(|r| r.to_string()).unwrap_or_default(),
                mark: Some(if a.emergency() { ui::WARN } else { ui::ACCENT }),
            }
        })
        .collect();
    ui::list(fb, &items, &l.ls);
    ui::hints(fb, &[("OK", "detail"), ("Lt/Rt", "ch"), ("Tri", "hop"), ("Sq", if l.pcap.is_some() { "stop rec" } else { "rec" }), ("X", "stop")]);
}


fn draw_tx(fb: &mut zpod_ui::Framebuffer, st: &zpod_core::StatusInfo, tp: &TxPhase) {
    match tp {
        TxPhase::Warn { iface, .. } => {
            ui::header(fb, st, "Wi-Fi Lab", "TX WARN", ui::WARN, iface);
            ui::message(
                fb,
                "Before EVERY transmit",
                &[
                    zpod_labd::proto::TX_SAFEGUARD,
                    "Only a labelled TEST aircraft is sent.",
                    "OK: I confirm isolation.  Triangle: arm + TX 15s.",
                    "X: cancel. Never armed at boot.",
                ],
            );
            ui::hints(fb, &[("OK", "confirm"), ("Tri", "arm+TX"), ("X", "cancel")]);
        }
        TxPhase::Running { iface, chan, note } => {
            ui::header(fb, st, "Wi-Fi Lab", "TX", ui::WARN, &format!("{iface} ch {chan}"));
            ui::message(fb, "Transmitting TEST Remote ID", &[note, "Moving TEST aircraft (not a UA).", "Adapter returns to managed when done."]);
            ui::hints(fb, &[("…", "sending")]);
        }
        TxPhase::Done { note } => {
            ui::header(fb, st, "Wi-Fi Lab", "TX done", ui::OK, "");
            ui::message(fb, "Transmit finished", &[note, "Arm was consumed. Next TX needs a new confirm."]);
            ui::hints(fb, &[("OK/X", "back")]);
        }
    }
}
