//! ZPOD launcher: the one LCD owner between apps.
//!
//! * Top menu: installed apps (from /opt/zpod/apps/*/manifest.toml), File
//!   transfer, Tools > (Wi-Fi / Bluetooth scanners, Wi-Fi info), Settings >,
//!   Power >. Multi-level menu tree in
//!   [`menu`]; button conventions in docs/ui/BUTTONS-AND-MENUS.md:
//!   Up/Down move, Right/OK/Circle enter, Left/X back one level, Home = top
//!   menu, Play/Pause = music (never a test tone), Vol+/- = volume overlay.
//! * Launching: draw a launch card, release spidev/GPIO, run the app as the
//!   unprivileged app user in its own process group, wait. Holding Home for
//!   3 s kills a hung app. Afterwards re-acquire the panel (warm, no flash).
//! * SIGTERM: M3 end screen ("ZPOD off / Safe to power off", "Rebooting...",
//!   or blank+backlight off) chosen by /etc/zpod/ui.conf [shutdown] screen=.
//! * Alarm / timer (M5): set in the Clock app, rung here ([`ring`]), even
//!   over a running app.
//! * Root-only helpers live here on purpose (the launcher is the privileged
//!   process): Wi-Fi/Bluetooth scans ([`tools`]), rsync share toggle and
//!   library rescan ([`transfer`]), date/RTC, power.
//! * Control socket `/run/zpod/launcher.sock` ([`ctl`], protocol in
//!   `zpod_apps::ctl`): launch / restart / stop / status from `zpod-app` over
//!   ssh (dev loop, docs/DEV-LOOP.md). Each app's stdout/stderr goes to its
//!   own log, `/data/zpod/logs/<id>.log` (`zpod_apps::logs`).
//! * `zpod-launcher --end-screen poweroff|reboot|off` draws just that frame
//!   (for BusyBox shutdown scripts).
//! * test3: Top = apps, Tools >, Settings >, Power >. Tools = Wi-Fi (one
//!   screen: status card, Off/Client/Hotspot, networks, saved, hotspot
//!   devices and password, Wi-Fi tools), Bluetooth, USB, Battery, File
//!   transfer. Play/Pause toggles music on the top menu only; elsewhere it
//!   only pauses music that is playing.
//! * M6: Battery page (history from zpod-battery), About split from it,
//!   Wi-Fi tools (signal meter, channels, saved networks, hotspot), Drop box
//!   and Book catalog switches ([`server`]), and a first-screen notice when
//!   bootfs setup files were applied after the first boot.

mod coreupd;
mod ctl;
mod icons;
mod menu;
mod pages;
mod proc;
mod ring;
mod server;
mod tools;
mod transfer;
mod usb;
mod usbrole;
mod wifitools;

use std::path::PathBuf;
use std::process::Child;
use std::time::{Duration, Instant};

use embedded_graphics::{pixelcolor::Rgb565, prelude::*, primitives::*};
use menu::{Effect, Item, MenuId, Menus, Page};
use proc::StopKind;
use zpod_apps::AppInfo;
use zpod_core::{classify, DateTime, Event, Key, KeyState, Nav, PlayState, StatusInfo};
use zpod_hal::{alarm, battery, conf, lcd, storage, sys, wifi, Platform};
use zpod_ui::{
    fill, rect,
    status::draw_status_bar,
    text::{self, Align},
    theme::*,
    widgets::{self, Row, HINTS_H},
    Font,
};

const LIST_Y: i32 = STATUS_H;
const LIST_H: i32 = 240 - STATUS_H - HINTS_H;
const HOME_ROW: i32 = 40;
const SUB_ROW: i32 = 30;
const APPS_ROW: i32 = 34;
const KILL_HOLD: Duration = Duration::from_secs(3);
const DROP_PORT: u16 = 8080;
const SPI_KEEP_WINDOW: Duration = Duration::from_secs(10);

/// Date & time editor (Settings > Date & time). App-local bindings:
/// Up/Down change the field, Right next field, Left previous field (Left on
/// the first field = back), OK/Circle save (date + `hwclock -w`), X cancel.
#[derive(Clone, Debug, PartialEq)]
struct TimeEdit {
    t: DateTime,
    field: usize,
}

const TIME_FIELDS: [&str; 5] = ["Year", "Month", "Day", "Hour", "Minute"];

fn days_in_month(y: i32, m: u8) -> u8 {
    match m {
        2 if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

impl TimeEdit {
    fn new(now: DateTime) -> Self {
        TimeEdit { t: DateTime { second: 0, ..now }, field: 0 }
    }
    fn adjust(&mut self, d: i32) {
        let t = &mut self.t;
        let wrap = |v: i32, lo: i32, hi: i32| lo + (v - lo).rem_euclid(hi - lo + 1);
        match self.field {
            0 => t.year = (t.year + d).clamp(2024, 2099),
            1 => t.month = wrap(t.month as i32 + d, 1, 12) as u8,
            2 => t.day = wrap(t.day as i32 + d, 1, days_in_month(t.year, t.month) as i32) as u8,
            3 => t.hour = wrap(t.hour as i32 + d, 0, 23) as u8,
            _ => t.minute = wrap(t.minute as i32 + d, 0, 59) as u8,
        }
        t.day = t.day.min(days_in_month(t.year, t.month));
    }
}

/// Diagnostics > Button test: shows every button as it is pressed/held.
/// Every key is captured (Left/X do not go back); Home exits (press it last).
#[derive(Clone, Debug, Default, PartialEq)]
struct ButtonTest {
    held: Vec<Key>,
    seen: Vec<Key>,
    last: Option<(Key, KeyState)>,
    repeats: u32,
}

const ALL_KEYS: [(Key, &str); 13] = [
    (Key::Up, "Up"),
    (Key::Down, "Down"),
    (Key::Left, "Left"),
    (Key::Right, "Right"),
    (Key::Ok, "Centre"),
    (Key::Home, "Home"),
    (Key::PlayPause, "Play"),
    (Key::Triangle, "Tri (I)"),
    (Key::Square, "Sq (U)"),
    (Key::Circle, "Circ (K)"),
    (Key::Cross, "X (J)"),
    (Key::VolUp, "Vol+"),
    (Key::VolDown, "Vol-"),
];

/// Join network > password page state (keyboard reader lives only here).
pub struct PwEntry {
    pub ssid: String,
    /// keyboard or on-screen characters (zpod_ui::textentry)
    pub entry: zpod_ui::textentry::TextEntry,
    pub hid: zpod_input::HidInput,
    pub role: zpod_input::UsbRole,
    /// (ok, message) from the last attempt / validation
    pub note: Option<(bool, String)>,
}

impl PwEntry {
    fn new(ssid: String) -> Self {
        PwEntry { ssid, entry: zpod_ui::textentry::TextEntry::wifi_password(), hid: zpod_input::HidInput::spawn(), role: zpod_input::role::usb_role(), note: None }
    }
}

struct Launcher {
    p: Platform,
    menus: Menus,
    apps: Vec<AppInfo>,
    broken: Vec<(String, String)>,
    /// the menu tree (docs/APP-MANIFEST.md): where each app is listed
    tree: zpod_apps::tree::Tree,
    icons: Vec<icons::Icon>,
    system_icon: icons::Icon,
    folder_icons: Vec<(&'static str, icons::Icon)>,
    /// Tools > Updates
    core: coreupd::Info,
    apps_dir: PathBuf,
    apps_stamp: Option<std::time::SystemTime>,
    status: StatusInfo,
    gauge: sys::Gauge,
    toast: Option<(String, Instant)>,
    tone: Option<Child>,
    /// test tone stops itself at this time (no `timeout` binary needed)
    tone_until: Option<Instant>,
    redraw: bool,
    app_user: Option<proc::AppUser>,
    wifi: sys::WifiInfo,
    time_edit: TimeEdit,
    buttons: ButtonTest,
    /// SPI clock trial: (previous MHz, revert deadline).
    spi_trial: Option<(u32, Instant)>,
    tools: tools::Tools,
    xfer: transfer::Info,
    xfer_at: Instant,
    ringer: Option<ring::Ringer>,
    alarm_checked: Instant,
    wt: wifitools::WifiTools,
    pw: Option<PwEntry>,
    batt: Option<pages::BattView>,
    batt_at: Instant,
    srv: server::State,
    srv_at: Instant,
    notice: Vec<String>,
    usb: usb::Info,
    usb_at: Instant,
    /// zpod-usb-role status (Tools > USB)
    role: zpod_input::role::RoleInfo,
    /// a USB role switch running on a thread
    usb_job: Option<usbrole::Job>,
    /// Control socket requests (None: socket not available, e.g. sim).
    ctl: Option<std::sync::mpsc::Receiver<ctl::Pending>>,
    /// App to start on the next main-loop turn (control socket / restart).
    pending_launch: Option<String>,
}

/// First-boot setup files applied on a later boot (written by
/// zpod-firstuser.sh / zpod-wifi-import.sh, one line each).
const NOTICE: &str = "/run/zpod/boot-notice";
/// Exists after the first boot reached the launcher (the scripts above only
/// report "late" files once this is there).
const BOOTED_ONCE: &str = "/var/lib/zpod/booted-once";

fn notice_path() -> PathBuf {
    std::env::var_os("ZPOD_NOTICE").map(Into::into).unwrap_or_else(|| NOTICE.into())
}

fn dir_stamp(p: &std::path::Path) -> Option<std::time::SystemTime> {
    let a = std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let b = std::fs::metadata("/run/zpod/apps-changed").and_then(|m| m.modified()).ok();
    a.max(b)
}

fn spi_mhz_now() -> u32 {
    lcd::spi_hz() / 1_000_000
}

fn low_batt_pct() -> f32 {
    conf::get("battery", "shutdown_pct").and_then(|s| s.parse().ok()).unwrap_or(5.0)
}

impl Launcher {
    fn new(p: Platform) -> Self {
        let apps_dir = zpod_apps::apps_dir();
        let app_user = if unsafe { libc::geteuid() } == 0 {
            let name = std::env::var("ZPOD_APP_USER").unwrap_or_else(|_| "zpod".into());
            let u = proc::lookup_user(&name);
            if u.is_none() {
                eprintln!("launcher: app user '{name}' not found; apps will run as root");
            }
            u
        } else {
            None
        };
        let mut l = Launcher {
            p,
            menus: Menus::new(),
            apps: Vec::new(),
            broken: Vec::new(),
            tree: Default::default(),
            icons: Vec::new(),
            system_icon: icons::from_png(icons::SYSTEM_PNG).unwrap(),
            folder_icons: icons::folders(),
            core: Default::default(),
            apps_dir,
            apps_stamp: None,
            status: StatusInfo::default(),
            gauge: sys::Gauge::default(),
            toast: None,
            tone: None,
            tone_until: None,
            redraw: true,
            app_user,
            wifi: sys::WifiInfo::default(),
            time_edit: TimeEdit::new(sys::now_local()),
            buttons: ButtonTest::default(),
            spi_trial: None,
            tools: tools::Tools::default(),
            xfer: transfer::Info::default(),
            xfer_at: Instant::now(),
            ringer: None,
            alarm_checked: Instant::now() - Duration::from_secs(5),
            wt: wifitools::WifiTools::default(),
            pw: None,
            batt: None,
            batt_at: Instant::now(),
            srv: server::State::default(),
            srv_at: Instant::now(),
            notice: Vec::new(),
            usb: usb::Info::default(),
            usb_at: Instant::now(),
            role: Default::default(),
            usb_job: None,
            ctl: None,
            pending_launch: None,
        };
        if !sys::sim() || std::env::var_os("ZPOD_SOCKET").is_some() {
            l.ctl = ctl::start_both(&zpod_apps::ctl::socket_path(), &zpod_apps::ctl::app_socket_path());
        }
        l.rescan(true);
        l.notice = std::fs::read_to_string(notice_path())
            .map(|t| t.lines().map(str::trim).filter(|x| !x.is_empty()).map(str::to_string).collect())
            .unwrap_or_default();
        if !l.notice.is_empty() {
            l.menus.push(Page::Notice);
        }
        if !sys::sim() && !std::path::Path::new(BOOTED_ONCE).exists() {
            let _ = std::fs::create_dir_all("/var/lib/zpod");
            let _ = std::fs::write(BOOTED_ONCE, format!("{}\n", sys::unix_now()));
        }
        l
    }

    fn load_battery(&mut self) {
        let (samples, sessions) = battery::load();
        self.batt = Some(pages::BattView { now: self.gauge.read_rate(), samples, sessions, unix_now: sys::unix_now(), low_pct: low_batt_pct() });
        self.batt_at = Instant::now();
        self.redraw = true;
    }

    fn refresh_server(&mut self) {
        self.srv = server::state();
        self.srv_at = Instant::now();
        self.redraw = true;
    }

    fn dismiss_notice(&mut self) {
        self.notice.clear();
        if !sys::sim() {
            let _ = std::fs::create_dir_all("/var/lib/zpod");
            let _ = std::fs::rename(notice_path(), "/var/lib/zpod/boot-notice.last");
        }
    }

    fn rescan(&mut self, force: bool) {
        let stamp = dir_stamp(&self.apps_dir);
        if !force && stamp == self.apps_stamp {
            return;
        }
        self.apps_stamp = stamp;
        let (apps, broken) = zpod_apps::discover(&self.apps_dir);
        for (id, why) in &broken {
            eprintln!("launcher: skipping app '{id}': {why}");
        }
        let tree = zpod_apps::tree::Tree::build(&apps);
        for (id, why) in &tree.notes {
            eprintln!("launcher: app '{id}': {why}");
        }
        // a folder page whose folder moved or went away: back out of it
        let same = |i: u16| self.tree.nodes.get(i as usize).map(|n| &n.path) == tree.nodes.get(i as usize).map(|n| &n.path) && tree.nodes.get(i as usize).is_some_and(|n| !n.entries.is_empty());
        if let Some(k) = self.menus.stack.iter().position(|(p, _)| matches!(p, Page::Menu(MenuId::Folder(i)) if !same(*i))) {
            self.menus.stack.truncate(k.max(1));
        }
        self.icons = apps.iter().map(|a| icons::load(a.icon_path().as_deref())).collect();
        self.apps = apps;
        self.broken = broken;
        self.tree = tree;
        for k in 0..self.menus.stack.len() {
            if let Page::Menu(id) = self.menus.stack[k].0 {
                let n = menu::items(id, &self.tree).len();
                let vis = if matches!(id, MenuId::Top | MenuId::Folder(_)) { HOME_ROW } else { SUB_ROW };
                let ls = &mut self.menus.stack[k].1;
                if ls.sel >= n {
                    ls.sel = n.saturating_sub(1);
                }
                ls.scroll_into_view(n, (LIST_H / vis) as usize);
            }
        }
        let n = menu::items(MenuId::Top, &self.tree).len();
        if let Some((_, ls)) = self.menus.stack.first_mut() {
            ls.scroll_into_view(n, (LIST_H / HOME_ROW) as usize);
        }
        self.redraw = true;
    }

    /// Keyboard events and hotplug on the password page (main loop).
    fn pw_poll(&mut self) {
        use zpod_input::{HidEvent, KeyCode, SourceKind};
        use zpod_ui::textentry::{KeyIn, Outcome};
        let mut out = Outcome::None;
        let joining = self.wt.job.is_some();
        let can_switch = self.role.can_switch;
        let Some(pw) = self.pw.as_mut() else { return };
        while let Some(h) = pw.hid.poll() {
            let HidEvent::Input(e) = h else {
                self.redraw = true; // added / removed
                continue;
            };
            let Some(k) = e.key_down() else { continue };
            if joining {
                continue; // joining: ignore typing
            }
            let ki = match k {
                KeyCode::Enter => KeyIn::Enter,
                KeyCode::Esc => KeyIn::Esc,
                KeyCode::Backspace if e.mods.ctrl => KeyIn::ClearAll,
                KeyCode::Backspace => KeyIn::Backspace,
                KeyCode::Tab => KeyIn::Tab,
                _ => match e.text {
                    Some(c) => KeyIn::Char(c),
                    None => continue,
                },
            };
            let o = pw.entry.key(ki);
            if o != Outcome::None {
                self.redraw = true;
                if matches!(ki, KeyIn::Char(_) | KeyIn::Backspace | KeyIn::ClearAll) {
                    pw.note = None;
                }
            }
            if matches!(o, Outcome::Submit | Outcome::Cancel) {
                out = o;
                break;
            }
        }
        // hotplug: a keyboard that appears or goes away switches the view
        let kb = pw.hid.has(SourceKind::Keyboard);
        if pw.entry.set_keyboard(kb) {
            self.redraw = true;
        }
        let offer = !kb && !pw.role.can_host() && can_switch;
        if offer != pw.entry.pick.offer_keyboard {
            pw.entry.pick.set_offer_keyboard(offer);
            self.redraw = true;
        }
        self.pw_outcome(out);
    }

    /// Buttons on the password page.
    fn pw_pad(&mut self, ev: &Event) {
        use zpod_ui::textentry::{Outcome, Pad};
        let held = ev.state == KeyState::Repeat;
        let p = match ev.key {
            Key::Up => Pad::Up,
            Key::Down => Pad::Down,
            Key::Left => Pad::Left,
            Key::Right => Pad::Right,
            _ if held => return, // only the arrows repeat
            Key::Ok | Key::Circle => Pad::Select,
            Key::Cross => Pad::Back,
            Key::Square => Pad::NextSet,
            Key::Triangle => Pad::Reveal,
            _ => return,
        };
        let joining = self.wt.job.is_some();
        let Some(pw) = self.pw.as_mut() else { return };
        if joining && !matches!(p, Pad::Reveal) {
            return;
        }
        let o = pw.entry.pad(p);
        if o != Outcome::None {
            self.redraw = true;
            if matches!(p, Pad::Select | Pad::Back) {
                pw.note = None;
            }
        }
        self.pw_outcome(o);
    }

    fn pw_outcome(&mut self, o: zpod_ui::textentry::Outcome) {
        use zpod_ui::textentry::Outcome;
        match o {
            Outcome::Submit => self.pw_submit(),
            Outcome::Cancel => self.pw_close(),
            Outcome::WantKeyboard => {
                // the same confirmation as Tools > USB > Host; the entry
                // (and what was typed) stays underneath
                if !self.role.can_switch {
                    self.toast("This image cannot switch USB live", 2200);
                } else if self.usb_job.is_none() {
                    self.menus.push(Page::UsbConfirm(menu::UsbChoice::Host));
                }
                self.redraw = true;
            }
            _ => {}
        }
    }

    fn pw_submit(&mut self) {
        let Some(pw) = self.pw.as_mut() else { return };
        if self.wt.job.is_some() {
            return;
        }
        if let Err(e) = zpod_hal::wifijoin::check_passphrase(pw.entry.field.as_str()) {
            pw.note = Some((false, e.to_string()));
            return;
        }
        let pass = pw.entry.field.take();
        pw.note = None;
        let ssid = pw.ssid.clone();
        if let Some(m) = self.wt.join_password(ssid, pass) {
            self.toast(m, 1500);
        }
    }

    fn pw_close(&mut self) {
        self.pw = None;
        if self.menus.page() == Page::WifiPassword {
            self.menus.back();
        }
        self.redraw = true;
    }

    /// Label of app `i` in menu `id` (`[menu] label` may differ per folder).
    fn app_label(&self, id: MenuId, i: usize) -> &str {
        let n = menu::node_of(id, &self.tree).and_then(|n| self.tree.nodes.get(n));
        n.and_then(|n| {
            n.entries.iter().find_map(|e| match e {
                zpod_apps::tree::Entry::App { app, label, .. } if *app == i => Some(label.as_str()),
                _ => None,
            })
        })
        .unwrap_or(&self.apps[i].manifest.app.name)
    }

    /// Devices on the USB port with the app that handles each (Tools > USB).
    fn usb_devices(&self) -> Vec<String> {
        self.role
            .devices
            .iter()
            .map(|d| {
                let id = d.split_whitespace().next().unwrap_or("");
                match zpod_apps::tree::handlers(&self.apps, "usb-device", id).first() {
                    Some(a) => format!("{} ({d})", a.manifest.app.name),
                    None => d.clone(),
                }
            })
            .collect()
    }

    fn toast(&mut self, msg: impl Into<String>, ms: u64) {
        self.toast = Some((msg.into(), Instant::now() + Duration::from_millis(ms)));
        self.redraw = true;
    }

    fn rows_for(&self, page: Page) -> (usize, usize) {
        match page {
            Page::Menu(MenuId::Top | MenuId::Folder(_)) => (0, (LIST_H / HOME_ROW) as usize),
            Page::Updates => (1, 1),
            Page::Menu(_) => (0, (LIST_H / SUB_ROW) as usize),
            Page::Apps => (self.apps.len() + self.broken.len(), (LIST_H / APPS_ROW) as usize),
            Page::WifiScan => (self.tools.wifi.items.len(), (LIST_H / APPS_ROW) as usize),
            Page::BtScan => (self.tools.bt.items.len(), (LIST_H / APPS_ROW) as usize),
            Page::WifiSaved => (self.wt.saved.len(), (LIST_H / APPS_ROW) as usize),
            Page::Wifi => (if self.wt.status.is_some() { menu::wifi_rows(self.wt.mode()).len() } else { 0 }, 4),
            Page::WifiMode => (3, 5),
            Page::Usb => (3, 3),
            Page::HotspotDevices => (self.wt.status.as_ref().map(|s| s.clients.len()).unwrap_or(0), (LIST_H / APPS_ROW) as usize),
            Page::WifiDevices => (self.wt.neighbours.len() + self.tools.wifi.items.len(), ((LIST_H - 16) / APPS_ROW) as usize),
            _ => (0, 1),
        }
    }

    // ---------------------------------------------------------------- draw
    fn draw(&mut self) {
        let st = self.status.clone();
        let page = self.menus.page();
        if let Some(r) = &self.ringer {
            draw_ring(&mut self.p.fb, r.ring, &st);
            self.redraw = false;
            return;
        }
        let title = match page {
            Page::WifiScan if !self.tools.wifi.items.is_empty() => format!("Networks ({})", self.tools.wifi.items.len()),
            Page::BtScan if !self.tools.bt.items.is_empty() => format!("Bluetooth ({})", self.tools.bt.items.len()),
            Page::ScanDetail => self.tools.detail_title.clone(),
            Page::Menu(MenuId::Folder(f)) => self.tree.nodes.get(f as usize).map(|n| n.title.clone()).unwrap_or_default(),
            _ => menu::title(page).to_string(),
        };
        let labels: Vec<String> = match page {
            Page::Menu(id) => menu::items(id, &self.tree)
                .iter()
                .map(|it| match it {
                    Item::App(a) => self.app_label(id, *a).to_string(),
                    _ => String::new(),
                })
                .collect(),
            _ => Vec::new(),
        };
        let usb_devs = if matches!(page, Page::Usb) { self.usb_devices() } else { Vec::new() };
        let fb = &mut self.p.fb;
        draw_status_bar(fb, &title, &st);
        let area = rect(0, LIST_Y, 320, LIST_H as u32);
        match page {
            Page::Menu(id) => {
                let items = menu::items(id, &self.tree);
                let ls = *self.menus.list();
                let sel = ls.sel;
                let (apps, icons, sysi, tree, ficons) = (&self.apps, &self.icons, &self.system_icon, &self.tree, &self.folder_icons);
                let top = id == MenuId::Top;
                let big = matches!(id, MenuId::Top | MenuId::Folder(_));
                let row_h = if big { HOME_ROW } else { SUB_ROW };
                let fsubs: Vec<String> = items
                    .iter()
                    .map(|it| match it {
                        Item::Folder(f) => tree.nodes.get(*f as usize).map(|n| folder_sub(tree, apps, n)).unwrap_or_default(),
                        _ => String::new(),
                    })
                    .collect();
                widgets::draw_list(fb, area, row_h, &ls, items.len(), |i| {
                    let it = items[i];
                    let (t, sub) = menu::label(it);
                    let mut r = match it {
                        Item::App(a) => {
                            let m = &apps[a].manifest.app;
                            let ic = if i == sel { &icons[a].selected } else { &icons[a].normal };
                            Row { title: &labels[i], sub: Some(&m.description), right: None, icon: big.then_some(ic), mark: None }
                        }
                        Item::Folder(f) => {
                            let n = &tree.nodes[f as usize];
                            let fi = ficons.iter().find(|(k, _)| *k == n.icon).or_else(|| ficons.iter().find(|(k, _)| *k == "folder")).map(|(_, i)| i).unwrap_or(sysi);
                            let ic = if i == sel { &fi.selected } else { &fi.normal };
                            Row { title: &n.title, sub: Some(&fsubs[i]), right: Some(">"), icon: big.then_some(ic), mark: None }
                        }
                        _ => {
                            let ic = if i == sel { &sysi.selected } else { &sysi.normal };
                            Row { title: t, sub: if sub.is_empty() { None } else { Some(sub) }, right: None, icon: if big { Some(ic) } else { None }, mark: None }
                        }
                    };
                    if matches!(it, Item::Menu(_)) {
                        r.right = Some(">");
                    }
                    if it == Item::Tone && self.tone.is_some() {
                        r.right = Some("playing");
                    }
                    if matches!(it, Item::Reboot | Item::Shutdown) {
                        r.mark = Some(WARN);
                    }
                    if !top && sub.is_empty() && !matches!(it, Item::App(_) | Item::Folder(_)) {
                        r.sub = None;
                    }
                    r
                });
                if top {
                    widgets::hints(fb, &[("OK/Rt", "open"), ("Play", "play/pause"), ("Vol", "volume")]);
                } else {
                    widgets::hints(fb, &[("OK/Rt", "open"), ("Lt/X", "back"), ("Home", "top")]);
                }
                if it_is_spi(&items, sel) {
                    let s = format!("now {} MHz", spi_mhz_now());
                    text::draw_aligned(fb, Font::Small, &s, 300, LIST_Y + 4 + (sel as i32 - ls.top as i32) * row_h + 8, Align::Right, ACCENT);
                }
            }
            Page::Status => {
                let _ = fb.fill_solid(&area, BG);
                let t = sys::now_local();
                let batt = self.gauge.read();
                let mpd = match sys::mpd_play_state() {
                    PlayState::Playing => "playing",
                    PlayState::Paused => "paused",
                    PlayState::Stopped => "stopped",
                    PlayState::Unknown => "not running",
                };
                let rel = storage::release();
                let rows: [(&str, String); 10] = [
                    ("Time", format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", t.year, t.month, t.day, t.hour, t.minute, t.second)),
                    ("wlan0", sys::ipv4("wlan0").unwrap_or_else(|| "--".into())),
                    ("usb0", sys::ipv4("usb0").unwrap_or_else(|| "--".into())),
                    ("Battery", batt.map(|(p, v, c)| format!("{p:.0}%  {v:.2} V{}", if c { "  charging" } else { "" })).unwrap_or_else(|| "n/a".into())),
                    ("Volume", st.volume_pct.map(|v| format!("{v}%  (side buttons)")).unwrap_or_else(|| "n/a".into())),
                    ("DAC", if sys::card_present() || sys::sim() { "ok".into() } else { "missing".into() }),
                    ("Music", mpd.into()),
                    ("Uptime", sys::uptime()),
                    ("SD card", storage::sd_summary()),
                    ("Build", rel.brief()),
                ];
                for (i, (k, v)) in rows.iter().enumerate() {
                    let y = LIST_Y + 2 + i as i32 * 20;
                    text::draw(fb, Font::Small, k, 10, y + 3, DIM);
                    text::draw_fit(fb, Font::Body, v, 84, y, 230, if v == "missing" { WARN } else { FG });
                }
                widgets::hints(fb, &[("Lt/X", "back"), ("Home", "top")]);
            }
            Page::About => {
                let _ = fb.fill_solid(&area, BG);
                let rtc = sys::rtc_info();
                let kernel = std::fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_default();
                let host = std::fs::read_to_string("/proc/sys/kernel/hostname").unwrap_or_default();
                let rel = storage::release();
                let mut rows: Vec<(&str, String)> = vec![
                    ("Version", if rel.is_known() { rel.brief() } else { "unknown (no /etc/zpod-release)".into() }),
                    ("SD card", storage::sd_card_bytes().map(|b| storage::human_card(b)).unwrap_or_else(|| "n/a".into())),
                ];
                for (k, v, _) in storage::partition_lines() {
                    rows.push((k, v));
                }
                rows.push(("Host", format!("{}  login {}", host.trim(), storage::login_line())));
                rows.push(("RTC", rtc.map(|(n, t)| format!("{n}  {t} UTC")).unwrap_or_else(|| "none (DS3231 0x68 not bound)".into())));
                rows.push(("Kernel", kernel.trim().to_string()));
                rows.push(("Core", format!("zpod {}  ({} apps)", zpod_apps::core_version(), self.apps.len())));
                let dy = if rows.len() > 8 { 20 } else { 24 };
                for (i, (k, v)) in rows.iter().enumerate().take(10) {
                    let y = LIST_Y + 5 + i as i32 * dy;
                    text::draw(fb, Font::Small, k, 10, y + 3, DIM);
                    text::draw_fit(fb, Font::Body, v, 84, y, 230, FG);
                }
                widgets::hints(fb, &[("Lt/X", "back"), ("Home", "top"), ("", "battery: top menu > Battery")]);
            }
            Page::Battery => match &self.batt {
                Some(b) => pages::draw_battery(fb, b),
                None => {
                    let _ = fb.fill_solid(&area, BG);
                }
            },
            Page::WifiSignal => pages::draw_meter(fb, &self.wt),
            Page::WifiPassword => match &self.pw {
                Some(pw) => pages::draw_password(fb, pw, self.wt.job.as_ref().map(|j| j.what.as_str())),
                None => {}
            },
            Page::WifiChannels => {
                let age = self.tools.wifi.age_text();
                pages::draw_chan_util(fb, &self.tools.wifi.items, self.tools.wifi.busy.is_some(), &age);
            }
            Page::WifiDevices => {
                let ls = *self.menus.list();
                pages::draw_addresses(fb, &self.wt.neighbours, &self.tools.wifi.items, &ls, self.tools.wifi.busy.is_some());
            }
            Page::WifiMode => {
                let ls = *self.menus.list();
                pages::draw_mode(fb, self.wt.mode(), &ls, self.wt.job.as_ref().map(|j| j.what.as_str()));
            }
            Page::HotspotDevices => {
                let ls = *self.menus.list();
                let empty = Vec::new();
                let peers = self.wt.status.as_ref().map(|s| &s.clients).unwrap_or(&empty);
                let ssid = self.wt.hotspot.as_ref().map(|h| h.ssid.clone()).unwrap_or_else(|| "the ZPOD hotspot".into());
                pages::draw_peers(fb, peers, &ssid, &ls);
            }
            Page::Usb => {
                let busy = self.usb_job.as_ref().map(|j| format!("Switching USB... {} s", j.started.elapsed().as_secs()));
                pages::draw_usb(fb, &self.usb, &self.role, &usb_devs, self.menus.list().sel, busy.as_deref());
            }
            Page::UsbConfirm(c) => {
                let busy = self.usb_job.as_ref().map(|j| format!("Switching USB... {} s", j.started.elapsed().as_secs()));
                pages::draw_usb_confirm(fb, c, &self.role, busy.as_deref());
            }
            Page::WifiSaved => {
                let ls = *self.menus.list();
                let w = &self.wt;
                if w.saved.is_empty() {
                    let _ = fb.fill_solid(&area, BG);
                    let msg = w.saved_err.clone().map(|e| format!("nmcli: {e}")).unwrap_or_else(|| "No saved Wi-Fi networks. Add one with zpod-wifi.txt.".into());
                    text::draw_fit(fb, Font::Body, &msg, 12, LIST_Y + 80, 296, if w.saved_err.is_some() { WARN } else { DIM });
                } else {
                    let subs: Vec<String> = w.saved.iter().map(|n| if n.active { "connected now".to_string() } else if n.autoconnect { "joins automatically when in range".to_string() } else { "manual".to_string() }).collect();
                    widgets::draw_list(fb, area, APPS_ROW, &ls, w.saved.len(), |i| Row {
                        title: &w.saved[i].name,
                        sub: Some(&subs[i]),
                        right: if w.saved[i].active { Some("on") } else { None },
                        icon: None,
                        mark: if w.saved[i].active { Some(CYAN) } else { None },
                    });
                }
                if let Some(j) = &w.job {
                    widgets::toast(fb, &j.what);
                }
                if let Some((i, _)) = w.forget_armed {
                    if let Some(n) = w.saved.get(i) {
                        widgets::toast(fb, &format!("Tri again: forget {}?", text::fit(Font::Body, &n.name, 90)));
                    }
                }
                widgets::hints(fb, &[("OK", "details"), ("Tri", "forget"), ("Sq", "refresh"), ("Lt/X", "back")]);
            }
            Page::Hotspot => {
                let home = self.wt.home_ssid.clone().or_else(|| self.wifi.ssid.clone());
                pages::draw_hotspot(fb, self.wt.hotspot.as_ref(), self.wt.job.as_ref().map(|j| j.what.as_str()), home.as_deref(), &self.srv, self.wt.reveal);
            }
            Page::Server(opds) => {
                let hs = self.wt.hotspot.as_ref().is_some_and(|h| h.active);
                pages::draw_server(fb, opds, &self.srv, hs);
            }
            Page::Notice => pages::draw_notice(fb, &self.notice),
            Page::Updates => pages::draw_updates(fb, &self.core, true),
            Page::Storage => {
                let _ = fb.fill_solid(&area, BG);
                let rel = storage::release();
                let dash = |v: &str| if v.is_empty() { "--".to_string() } else { v.to_string() };
                let mut rows: Vec<(&str, String, bool)> = vec![
                    ("Version", if rel.is_known() { dash(&rel.version) } else { "unknown (no /etc/zpod-release)".into() }, false),
                    ("Git", dash(&rel.git), false),
                    ("Built", dash(&rel.date), false),
                    ("Image", dash(rel.image_name()), false),
                    ("SD card", storage::sd_card_bytes().map(|b| format!("{}  ({} raw)", storage::human_card(b), storage::human(b))).unwrap_or_else(|| "n/a".into()), false),
                ];
                rows.extend(storage::partition_lines());
                let login = storage::login_line();
                let bad = login.starts_with("NOT SET UP");
                rows.push(("Login", login, bad));
                let dy = if rows.len() > 9 { 20 } else { 22 };
                for (i, (k, v, warn)) in rows.iter().enumerate() {
                    let y = LIST_Y + 3 + i as i32 * dy;
                    text::draw(fb, Font::Small, k, 10, y + 3, DIM);
                    text::draw_fit(fb, Font::Body, v, 84, y, 230, if *warn { WARN } else { FG });
                }
                widgets::hints(fb, &[("Lt/X", "back"), ("Home", "top")]);
            }
            Page::Wifi => {
                let ls = *self.menus.list();
                let rows = menu::wifi_rows(self.wt.mode());
                pages::draw_wifi(fb, &self.wt, &rows, &ls, self.wt.saved.len(), self.wt.job.as_ref().map(|j| j.what.as_str()));
            }
            Page::Transfer => draw_transfer(fb, &self.xfer),
            Page::WifiScan => {
                let t = &self.tools;
                let ls = *self.menus.list();
                let items = &t.wifi.items;
                let rows: Vec<(String, String, String)> = items
                    .iter()
                    .map(|n| {
                        let name = if n.ssid.is_empty() { "(hidden)".to_string() } else { n.ssid.clone() };
                        (name, format!("ch {}  {}  {}", n.channel, n.security, n.bssid), format!("{:.0} dBm", n.signal_dbm))
                    })
                    .collect();
                if rows.is_empty() {
                    let _ = fb.fill_solid(&area, BG);
                    let msg = match (&t.wifi.busy, &t.wifi.err) {
                        (Some(_), _) => "Scanning for networks...".to_string(),
                        (None, Some(e)) => format!("Scan failed: {e}"),
                        _ => "No networks found. Square = scan".to_string(),
                    };
                    text::draw_fit(fb, Font::Body, &msg, 12, LIST_Y + 80, 296, if t.wifi.err.is_some() { WARN } else { DIM });
                } else {
                    widgets::draw_list(fb, area, APPS_ROW, &ls, rows.len(), |i| Row {
                        title: &rows[i].0,
                        sub: Some(&rows[i].1),
                        right: Some(&rows[i].2),
                        icon: None,
                        mark: Some(signal_colour(items[i].signal_dbm, items[i].associated)),
                    });
                }
                let sort = format!("sort: {}", t.wifi_sort.label());
                let age = t.wifi.age_text();
                widgets::hints(fb, &[("OK", "details/join"), ("Sq", if t.wifi.busy.is_some() { "scanning" } else { "rescan" }), ("Tri", &sort), ("", &age)]);
            }
            Page::BtScan => {
                let t = &self.tools;
                let ls = *self.menus.list();
                let items = &t.bt.items;
                let rows: Vec<(String, String, String)> = items
                    .iter()
                    .map(|d| {
                        let name = if d.name.is_empty() { "(no name)".to_string() } else { d.name.clone() };
                        let kind = if d.kind.is_empty() { "?".to_string() } else { d.kind.clone() };
                        (name, format!("{}  {kind}", d.addr), d.rssi.map(|r| format!("{r} dBm")).unwrap_or_else(|| "--".into()))
                    })
                    .collect();
                if rows.is_empty() {
                    let _ = fb.fill_solid(&area, BG);
                    let msg = match (&t.bt.busy, &t.bt.err) {
                        (Some(_), _) => "Discovering devices (about 10 s)...".to_string(),
                        (None, Some(e)) => format!("Scan failed: {e}"),
                        _ => "No devices found. Square = scan".to_string(),
                    };
                    text::draw_fit(fb, Font::Body, &msg, 12, LIST_Y + 80, 296, if t.bt.err.is_some() { WARN } else { DIM });
                    text::draw_fit(fb, Font::Small, "Classic + BLE. Devices must be discoverable/advertising.", 12, LIST_Y + 104, 296, FAINT);
                } else {
                    widgets::draw_list(fb, area, APPS_ROW, &ls, rows.len(), |i| Row {
                        title: &rows[i].0,
                        sub: Some(&rows[i].1),
                        right: Some(&rows[i].2),
                        icon: None,
                        mark: Some(signal_colour(items[i].rssi.unwrap_or(-127) as f32, false)),
                    });
                }
                let sort = format!("sort: {}", if t.bt_by_name { "name" } else { "signal" });
                let age = t.bt.age_text();
                widgets::hints(fb, &[("OK", "details"), ("Sq", if t.bt.busy.is_some() { "scanning" } else { "rescan" }), ("Tri", &sort), ("", &age)]);
            }
            Page::ScanDetail => {
                let _ = fb.fill_solid(&area, BG);
                for (i, (k, v)) in self.tools.detail.iter().enumerate() {
                    let y = LIST_Y + 8 + i as i32 * 26;
                    text::draw(fb, Font::Small, k, 12, y + 3, DIM);
                    text::draw_fit(fb, Font::Body, v, 84, y, 228, FG);
                }
                widgets::hints(fb, &[("Lt/X", "back"), ("Home", "top")]);
            }
            Page::Apps => {
                let n = self.apps.len() + self.broken.len();
                let (apps, broken, tree) = (&self.apps, &self.broken, &self.tree);
                let mut subs: Vec<String> = Vec::with_capacity(n);
                for (i, a) in apps.iter().enumerate() {
                    let id = &a.manifest.app.id;
                    let place = match tree.notes.iter().find(|(n, _)| n == id) {
                        Some((_, why)) => why.clone(),
                        None if tree.hidden.contains(&i) => "hidden".into(),
                        None => zpod_apps::tree::placements(&a.manifest)
                            .map(|ps| ps.iter().map(|p| if p.path.is_empty() { "Home".to_string() } else { p.path.clone() }).collect::<Vec<_>>().join(", "))
                            .unwrap_or_default(),
                    };
                    subs.push(format!("{id}  \u{b7}  {place}"));
                }
                for (_, why) in broken {
                    subs.push(why.clone());
                }
                let ls = *self.menus.list();
                widgets::draw_list(fb, area, APPS_ROW, &ls, n, |i| {
                    if i < apps.len() {
                        let a = &apps[i].manifest.app;
                        let warn = tree.hidden.contains(&i) || tree.notes.iter().any(|(n, _)| *n == a.id);
                        Row { title: &a.name, sub: Some(&subs[i]), right: Some(&a.version), icon: None, mark: if warn { Some(WARN) } else { None } }
                    } else {
                        let (id, _) = &broken[i - apps.len()];
                        Row { title: id, sub: Some(&subs[i]), right: Some("BROKEN"), icon: None, mark: Some(WARN) }
                    }
                });
                widgets::hints(fb, &[("Lt/X", "back"), ("", "zpod-app install x.zpa  \u{b7}  zpod-app tree")]);
            }
            Page::DateTime => {
                let _ = fb.fill_solid(&area, BG);
                let e = &self.time_edit;
                let vals = [
                    format!("{:04}", e.t.year),
                    format!("{:02}", e.t.month),
                    format!("{:02}", e.t.day),
                    format!("{:02}", e.t.hour),
                    format!("{:02}", e.t.minute),
                ];
                // lay out "YYYY-MM-DD  HH:MM" from measured glyph widths
                let seps = ["-", "-", "  ", ":", ""];
                let total: i32 = (0..5).map(|i| text::width(Font::Huge, &vals[i]) + text::width(Font::Huge, seps[i]) + 4).sum();
                let mut x = (320 - total) / 2;
                for i in 0..5 {
                    let selc = i == e.field;
                    let w = text::width(Font::Huge, &vals[i]);
                    if selc {
                        fill(fb, x - 3, LIST_Y + 50, (w + 6) as u32, 34, SEL);
                    }
                    text::draw(fb, Font::Huge, &vals[i], x, LIST_Y + 54, if selc { WHITE } else { FG });
                    text::draw(fb, Font::Tiny, TIME_FIELDS[i], x, LIST_Y + 90, DIM);
                    x += w + 2;
                    text::draw(fb, Font::Huge, seps[i], x, LIST_Y + 54, DIM);
                    x += text::width(Font::Huge, seps[i]) + 2;
                }
                text::draw_aligned(fb, Font::Body, "Local time. OK = set clock + save to RTC", 160, LIST_Y + 116, Align::Center, DIM);
                let rtc = sys::rtc_info().map(|(n, t)| format!("RTC {n}: {t} UTC")).unwrap_or_else(|| "No RTC bound (DS3231 0x68)".into());
                text::draw_aligned(fb, Font::Small, &rtc, 160, LIST_Y + 138, Align::Center, FAINT);
                widgets::hints(fb, &[("Up/Dn", "change"), ("Lt/Rt", "field"), ("OK", "save"), ("X", "cancel")]);
            }
            Page::ButtonTest => {
                let _ = fb.fill_solid(&area, BG);
                let b = &self.buttons;
                for (i, (k, name)) in ALL_KEYS.iter().enumerate() {
                    let (cx, cy) = (8 + (i as i32 % 4) * 78, LIST_Y + 8 + (i as i32 / 4) * 34);
                    let held = b.held.contains(k);
                    let seen = b.seen.contains(k);
                    let bg = if held { OK } else if seen { SEL } else { PANEL };
                    fill(fb, cx, cy, 72, 28, bg);
                    text::draw_aligned(fb, Font::BodyBold, name, cx + 36, cy + 8, Align::Center, if held { BLACK } else { FG });
                }
                let last = b.last.map(|(k, s)| format!("last: {:?} {:?}   repeats: {}   seen {}/13", k, s, b.repeats, b.seen.len())).unwrap_or_else(|| "press any button".into());
                text::draw_aligned(fb, Font::Small, &last, 160, LIST_Y + 150, Align::Center, DIM);
                widgets::hints(fb, &[("", "every key is captured"), ("Home", "exit (press it last)")]);
            }
            Page::Confirm(reboot) => {
                let (t, l1) = if reboot {
                    ("Reboot?", "The ZPOD restarts (about 90 s).")
                } else {
                    ("Shut down?", "Wait for \"Safe to power off\".")
                };
                let _ = fb.fill_solid(&area, BG);
                widgets::dialog(fb, t, &[l1, "OK = yes      Left/X = cancel"], WARN);
                widgets::hints(fb, &[("OK", "yes"), ("Lt/X", "cancel")]);
            }
        }
        if let Some((msg, _)) = &self.toast {
            widgets::toast(fb, msg);
        }
        self.redraw = false;
    }

    /// Alarm/timer: ring when due (checked about once a second).
    fn check_alarm(&mut self) {
        if self.ringer.is_some() || self.alarm_checked.elapsed() < Duration::from_millis(900) {
            return;
        }
        self.alarm_checked = Instant::now();
        let a = alarm::load();
        if let Some(r) = a.due(&sys::now_local(), sys::unix_now()) {
            eprintln!("launcher: ringing {r:?}");
            if let Some(mut t) = self.tone.take() {
                proc::stop_child(&mut t, Duration::from_millis(300));
            }
            self.ringer = Some(ring::Ringer::start(r));
            self.p.status.set_play(PlayState::Paused);
            self.p.fb.mark_all_dirty();
            self.redraw = true;
        }
    }

    fn alarm_due_soon(&self) -> bool {
        alarm::load().due(&sys::now_local(), sys::unix_now()).is_some()
    }

    /// End the ring: snooze (true) or stop.
    fn end_ring(&mut self, snooze: bool) {
        let Some(mut r) = self.ringer.take() else { return };
        r.stop();
        let mut a = alarm::load();
        let now = sys::unix_now();
        if snooze && matches!(r.ring, alarm::Ring::Alarm(_)) {
            a.snooze(r.ring, now);
            self.toast(format!("Snoozed {} min", a.snooze_min), 1800);
        } else {
            a.dismiss(r.ring);
            self.toast(if r.ring == alarm::Ring::Timer { "Timer stopped" } else { "Alarm off" }, 1500);
        }
        if let Err(e) = alarm::save(&a) {
            eprintln!("launcher: cannot save {}: {e}", alarm::path().display());
        }
        self.status = StatusInfo::default();
        self.p.fb.mark_all_dirty();
        self.redraw = true;
    }

    fn refresh_transfer(&mut self) {
        self.xfer = transfer::gather(drop_running());
        self.xfer_at = Instant::now();
        self.redraw = true;
    }

    fn busy(&mut self, msg: &str) {
        let fb = &mut self.p.fb;
        let area = rect(0, LIST_Y, 320, LIST_H as u32);
        let _ = fb.fill_solid(&area, BG);
        text::draw_aligned(fb, Font::Huge, msg, 160, 100, Align::Center, ACCENT);
        fill(fb, 0, 240 - HINTS_H, 320, HINTS_H as u32, BAR);
        self.p.flush();
    }

    // -------------------------------------------------------------- input
    fn key(&mut self, ev: Event) -> Option<Action> {
        if let Some(r) = &self.ringer {
            if ev.state == KeyState::Press {
                match ring::classify(r.ring, ev.key) {
                    ring::RingKey::Stop => self.end_ring(false),
                    ring::RingKey::Snooze => self.end_ring(true),
                    ring::RingKey::Ignore => {}
                }
            }
            return None;
        }
        let page = self.menus.page();
        // Diagnostics > Button test captures every key; Home (press) exits.
        if page == Page::ButtonTest {
            let b = &mut self.buttons;
            match ev.state {
                KeyState::Press => {
                    if !b.held.contains(&ev.key) {
                        b.held.push(ev.key);
                    }
                    if !b.seen.contains(&ev.key) {
                        b.seen.push(ev.key);
                    }
                }
                KeyState::Release => b.held.retain(|k| *k != ev.key),
                KeyState::Repeat => b.repeats += 1,
            }
            b.last = Some((ev.key, ev.state));
            self.redraw = true;
            if ev.key == Key::Home && ev.state == KeyState::Press {
                self.menus.back();
            }
            return None;
        }
        if page == Page::WifiPassword && ev.state != KeyState::Release && !matches!(ev.key, Key::Home | Key::VolUp | Key::VolDown | Key::PlayPause) {
            self.pw_pad(&ev);
            return None;
        }
        if let Page::UsbConfirm(c) = page {
            if ev.state == KeyState::Press && !matches!(ev.key, Key::Home | Key::VolUp | Key::VolDown | Key::PlayPause) {
                self.usb_confirm_key(c, ev.key);
                return None;
            }
            if ev.key != Key::Home {
                return None;
            }
        }
        if page == Page::WifiPassword && ev.key == Key::Home {
            self.pw = None; // wipes the field, stops the keyboard reader
        }
        let nav = classify(&ev)?;
        // SPI trial: OK (or any Enter) keeps the new clock.
        if let Some((_, _)) = self.spi_trial {
            if nav == Nav::Enter {
                let mhz = spi_mhz_now();
                let _ = conf::set("display", "spi_mhz", &mhz.to_string());
                self.spi_trial = None;
                self.toast(format!("SPI {mhz} MHz kept"), 1500);
                return None;
            }
        }
        if page == Page::DateTime && !matches!(nav, Nav::Home | Nav::PlayPause | Nav::Volume) {
            let e = &mut self.time_edit;
            self.redraw = true;
            match ev.key {
                Key::Up => e.adjust(1),
                Key::Down => e.adjust(-1),
                Key::Right => e.field = (e.field + 1).min(TIME_FIELDS.len() - 1),
                Key::Left if e.field > 0 => e.field -= 1,
                Key::Left | Key::Cross => self.menus.back(),
                Key::Ok | Key::Circle => {
                    let t = self.time_edit.t;
                    match sys::set_local_time(&t) {
                        Ok(()) => self.toast("Time set + saved to RTC", 1800),
                        Err(e) => self.toast(e, 2500),
                    }
                    self.menus.back();
                }
                _ => {}
            }
            return None;
        }
        // page-local keys: Square = rescan / share toggle, Triangle = sort
        match (page, &nav) {
            (Page::WifiScan, Nav::Other(Key::Square)) => {
                self.tools.wifi.start(tools::wifi_job);
                self.redraw = true;
                return None;
            }
            (Page::BtScan, Nav::Other(Key::Square)) => {
                self.tools.bt.start(tools::bt_job);
                self.redraw = true;
                return None;
            }
            (Page::WifiScan, Nav::Other(Key::Triangle)) => {
                self.tools.cycle_wifi_sort();
                self.menus.list_mut().sel = 0;
                self.menus.list_mut().top = 0;
                self.toast(format!("Sorted by {}", self.tools.wifi_sort.label()), 900);
                return None;
            }
            (Page::BtScan, Nav::Other(Key::Triangle)) => {
                self.tools.cycle_bt_sort();
                self.menus.list_mut().sel = 0;
                self.menus.list_mut().top = 0;
                self.toast(if self.tools.bt_by_name { "Sorted by name" } else { "Sorted by signal" }, 900);
                return None;
            }
            (Page::WifiChannels, Nav::Other(Key::Square)) => {
                self.tools.wifi.start(tools::wifi_job);
                self.redraw = true;
                return None;
            }
            (Page::Usb, Nav::Other(Key::Square)) => {
                self.refresh_usb();
                self.toast("Refreshed", 600);
                return None;
            }
            (Page::WifiSaved, Nav::Other(Key::Square)) => {
                self.wt.refresh_saved();
                self.redraw = true;
                return None;
            }
            (Page::WifiSaved, Nav::Other(Key::Triangle)) => {
                let sel = self.menus.list().sel;
                if let Some(m) = self.wt.forget(sel) {
                    self.toast(m, 1500);
                }
                self.redraw = true;
                return None;
            }
            (Page::Hotspot, Nav::Other(Key::Triangle)) => {
                self.wt.new_password();
                self.redraw = true;
                return None;
            }
            (Page::Transfer, Nav::Other(Key::Square)) => {
                match self.xfer.rsync {
                    None => self.toast("rsync share not installed", 1800),
                    Some(on) => match transfer::set_rsync(!on) {
                        Ok(()) => {
                            self.toast(if on { "rsync share off" } else { "rsync share on (USB only)" }, 1800);
                            if sys::sim() {
                                if on {
                                    std::env::remove_var("ZPOD_SIM_RSYNC")
                                } else {
                                    std::env::set_var("ZPOD_SIM_RSYNC", "1")
                                }
                            }
                        }
                        Err(e) => self.toast(e, 2500),
                    },
                }
                self.refresh_transfer();
                return None;
            }
            _ => {}
        }
        let rows = self.rows_for(page);
        let eff = self.menus.handle(nav, &self.tree, rows);
        self.redraw = true;
        match eff {
            Effect::None | Effect::Redraw => None,
            Effect::Launch(i) => Some(Action::Launch(i)),
            Effect::PlayPause => Some(Action::PlayPause),
            Effect::Tone => Some(Action::Tone),
            Effect::BacklightTest => Some(Action::Backlight),
            Effect::CycleSpi => Some(Action::CycleSpi),
            Effect::PauseIfPlaying => {
                self.pause_if_playing();
                None
            }
            Effect::Activate(p) => {
                match p {
                    Page::Wifi => {
                        let sel = self.menus.list().sel;
                        if let Some(r) = menu::wifi_rows(self.wt.mode()).get(sel).copied() {
                            if let Effect::Opened(p) = self.menus.open_wifi_row(r) {
                                self.opened(p);
                            }
                        }
                    }
                    Page::WifiMode => {
                        let m = wifi::Mode::ALL[self.menus.list().sel.min(2)];
                        let best = (!self.tools.wifi.items.is_empty()).then(|| wifi::best_channel(&wifi::congestion(&self.tools.wifi.items)));
                        match self.wt.set_mode(m, best) {
                            Some(msg) => self.toast(msg, 1500),
                            None => self.menus.back(), // card shows "working..." then the new mode
                        }
                    }
                    Page::WifiSignal => {
                        if let Some(m) = self.wt.join() {
                            self.toast(m, 2200);
                        } else if self.wt.want_password {
                            self.wt.want_password = false;
                            self.role = usbrole::status(); // can it offer USB host for a keyboard?
                            self.pw = Some(PwEntry::new(self.wt.detail.ssid.clone()));
                            self.menus.push(Page::WifiPassword);
                            self.redraw = true;
                        }
                    }
                    Page::Hotspot => self.wt.reveal = !self.wt.reveal,
                    Page::HotspotDevices => self.wt.refresh_status(true),
                    Page::WifiDevices | Page::WifiChannels => {
                        self.wt.neighbours = wifi::neighbours();
                        self.tools.wifi.start(tools::wifi_job);
                    }
                    Page::Usb => self.usb_row_ok(),
                    Page::Server(opds) => match server::toggle(opds) {
                        Ok(st) => {
                            let on = if opds { st.opds } else { st.dropbox };
                            self.srv = st;
                            self.toast(format!("{} {}", if opds { "Book catalog" } else { "Drop box" }, if on { "on" } else { "off" }), 1500);
                        }
                        Err(e) => self.toast(e, 2500),
                    },
                    Page::Notice => self.dismiss_notice(),
                    Page::Updates => {
                        if !self.core.boot_support && self.core.error.is_none() {
                            self.toast("test3 loads updates only by hand (zpod-dev --core)", 2500);
                        } else {
                            match coreupd::set(!self.core.boot_load) {
                                Ok(m) => self.toast(m, 2000),
                                Err(e) => self.toast(e, 2500),
                            }
                            self.core = coreupd::read();
                        }
                    }
                    _ => {}
                }
                None
            }
            Effect::RescanMusic => {
                match transfer::rescan() {
                    Ok(()) => self.toast("Rescanning music library", 1500),
                    Err(e) => self.toast(e, 2500),
                }
                self.refresh_transfer();
                if sys::sim() {
                    self.xfer.updating = true;
                }
                None
            }
            Effect::Opened(p) => {
                self.opened(p);
                None
            }
            Effect::Power(reboot) => Some(Action::Power(if reboot { StopKind::Reboot } else { StopKind::Poweroff })),
        }
    }

    /// Entered page `p`: load what it shows.
    fn opened(&mut self, p: Page) {
        match p {
            Page::Wifi => {
                self.wt.refresh_status(true);
                self.wt.refresh_saved();
            }
            Page::WifiMode => {
                let cur = self.wt.mode();
                self.menus.list_mut().sel = wifi::Mode::ALL.iter().position(|m| *m == cur).unwrap_or(1);
            }
            Page::HotspotDevices => self.wt.refresh_status(true),
            Page::WifiDevices => {
                self.wt.neighbours = wifi::neighbours();
                self.tools.wifi.start_if_stale(tools::wifi_job);
            }
            Page::Usb => self.refresh_usb(),
            Page::Updates => self.core = coreupd::read(),
            Page::Battery => self.load_battery(),
            Page::WifiSignal => self.open_details(),
            Page::WifiChannels => self.tools.wifi.start_if_stale(tools::wifi_job),
            Page::WifiSaved => self.wt.refresh_saved(),
            Page::Hotspot => {
                self.wt.reveal = false;
                self.wt.refresh_hotspot();
                self.wifi = sys::wifi_info();
                self.refresh_server();
            }
            Page::Server(_) => {
                self.wt.refresh_hotspot();
                self.refresh_server();
            }
            Page::Apps => self.rescan(true),
            Page::DateTime => self.time_edit = TimeEdit::new(sys::now_local()),
            Page::ButtonTest => self.buttons = ButtonTest::default(),
            Page::Transfer => self.refresh_transfer(),
            Page::WifiScan => self.tools.wifi.start_if_stale(tools::wifi_job),
            Page::BtScan => self.tools.bt.start_if_stale(tools::bt_job),
            Page::ScanDetail => {
                let sel = self.menus.list().sel;
                let parent = self.menus.stack.len().checked_sub(2).map(|i| self.menus.stack[i].0);
                let ok = match parent {
                    Some(Page::BtScan) => self.tools.bt_detail(sel),
                    _ => self.tools.wifi_detail(sel),
                };
                if !ok {
                    self.menus.back();
                }
            }
            _ => {}
        }
    }

    /// Network details (meter) for the scan row / saved network under the
    /// cursor of the page below, or the connected network.
    fn open_details(&mut self) {
        let sel = self.menus.list().sel;
        let parent = self.menus.stack.len().checked_sub(2).map(|i| self.menus.stack[i].0);
        let saved_uuid = |ssid: &str, w: &wifitools::WifiTools| w.saved.iter().find(|s| s.name == ssid).map(|s| s.uuid.clone());
        let (target, detail) = match parent {
            Some(Page::WifiScan) => match self.tools.wifi.items.get(sel) {
                Some(n) => (
                    (!n.associated).then(|| wifitools::Target { bssid: n.bssid.clone(), ssid: n.ssid.clone(), channel: n.channel }),
                    wifitools::NetDetail { ssid: n.ssid.clone(), security: n.security.clone(), saved_uuid: saved_uuid(&n.ssid, &self.wt), connected: n.associated },
                ),
                None => (None, wifitools::NetDetail::default()),
            },
            Some(Page::WifiSaved) => match self.wt.saved.get(sel) {
                Some(s) => {
                    let sec = self.tools.wifi.items.iter().find(|n| n.ssid == s.name).map(|n| n.security.clone()).unwrap_or_default();
                    (
                        (!s.active).then(|| wifitools::Target { bssid: String::new(), ssid: s.name.clone(), channel: 0 }),
                        wifitools::NetDetail { ssid: s.name.clone(), security: sec, saved_uuid: Some(s.uuid.clone()), connected: s.active },
                    )
                }
                None => (None, wifitools::NetDetail::default()),
            },
            _ => (None, wifitools::NetDetail { connected: true, ..Default::default() }),
        };
        self.wt.detail = detail;
        self.wt.open_meter(target);
    }

    fn refresh_usb(&mut self) {
        self.usb = usb::gather();
        self.role = usbrole::status();
        self.usb_at = Instant::now();
        self.redraw = true;
    }

    /// Tools > USB, OK on a row: open its confirmation.
    fn usb_row_ok(&mut self) {
        let sel = self.menus.list().sel.min(2);
        let c = menu::UsbChoice::ALL[sel];
        if self.usb_job.is_some() {
            return;
        }
        if !self.role.can_switch {
            self.toast("This image cannot switch live (no zpod-usb-role)", 2500);
        } else if usbrole::is_current(c.target(), self.role.role) {
            self.toast(format!("Already: {}", usbrole::target_label(c.target())), 1500);
        } else {
            self.menus.push(Page::UsbConfirm(c));
            self.redraw = true;
        }
    }

    /// Keys on the "Switch USB?" confirmation.
    fn usb_confirm_key(&mut self, c: menu::UsbChoice, k: Key) {
        self.redraw = true;
        let busy = self.usb_job.is_some();
        let unsafe_host = c == menu::UsbChoice::Host && !self.role.safe;
        match k {
            Key::Left | Key::Cross if !busy => self.menus.back(),
            Key::Ok | Key::Circle if !busy => {
                if unsafe_host {
                    self.toast("Needs Wi-Fi SSH (or Triangle)", 2200);
                } else {
                    self.usb_job = Some(usbrole::Job::start(c.target(), false, false));
                }
            }
            Key::Triangle if !busy && unsafe_host => {
                eprintln!("launcher: USB host without Wi-Fi SSH (confirmed on screen)");
                self.usb_job = Some(usbrole::Job::start(c.target(), true, false));
            }
            Key::Square if !busy && usbrole::can_persist(&self.role, c.target()) => {
                if unsafe_host {
                    self.toast("No Wi-Fi SSH: not saved for next boot", 2200);
                } else {
                    self.usb_job = Some(usbrole::Job::start(c.target(), false, true));
                }
            }
            _ => {}
        }
    }

    /// A finished Tools > USB switch.
    fn usb_job_poll(&mut self) {
        let Some(r) = self.usb_job.as_ref().and_then(|j| j.poll()) else { return };
        let to = self.usb_job.take().map(|j| j.to);
        match r {
            Ok(_) => self.toast(format!("USB: {}", to.map(usbrole::target_label).unwrap_or("switched")), 2200),
            Err(e) => self.toast(format!("USB switch failed: {e}"), 4000),
        }
        if matches!(self.menus.page(), Page::UsbConfirm(_)) {
            self.menus.back();
        }
        self.p.status.net_changed();
        self.refresh_usb();
        if let Some(pw) = self.pw.as_mut() {
            pw.role = zpod_input::role::usb_role();
        }
    }

    /// Play/Pause away from the top menu: pause music that is playing,
    /// otherwise do nothing (test2: Play in Tools said "Nothing queued").
    fn pause_if_playing(&mut self) {
        if self.tone.is_some() {
            self.stop_tone();
            return;
        }
        let playing = if sys::sim() { std::env::var("ZPOD_SIM_PLAY").as_deref() == Ok("play") } else { sys::mpd_play_state() == PlayState::Playing };
        if playing {
            let (st, msg) = sys::mpd_toggle();
            if st != PlayState::Unknown {
                self.p.status.set_play(st);
            }
            self.toast(msg, 1000);
        }
    }

    fn stop_tone(&mut self) {
        if let Some(mut c) = self.tone.take() {
            proc::stop_child(&mut c, Duration::from_millis(300));
        }
        self.tone_until = None;
    }

    /// Stop the test tone after 10 s (own deadline; the Buildroot image had
    /// no `timeout` applet in test2).
    fn tone_tick(&mut self) {
        if self.tone_until.is_some_and(|t| Instant::now() >= t) {
            self.stop_tone();
            self.redraw = true;
        } else if let Some(c) = self.tone.as_mut() {
            if let Ok(Some(_)) = c.try_wait() {
                self.tone = None;
                self.tone_until = None;
                self.redraw = true;
            }
        }
    }

    fn toggle_tone(&mut self) {
        if self.tone.is_some() {
            self.stop_tone();
            self.toast("Tone stopped", 1500);
            return;
        }
        // The DAC is exclusive: pause MPD first.
        if let Ok(mut c) = zpod_mpd::Client::connect_default() {
            let _ = c.simple("pause 1");
            self.p.status.set_play(PlayState::Paused);
        }
        let mut cmd = std::process::Command::new("speaker-test");
        cmd.args(["-D", "default", "-t", "sine", "-f", "440", "-c", "2"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
        match if sys::sim() { Err(std::io::Error::other("sim")) } else { cmd.spawn() } {
            Ok(c) => {
                self.tone = Some(c);
                self.tone_until = Some(Instant::now() + Duration::from_secs(10));
                self.toast("Tone 440 Hz (OK again = stop)", 1500);
            }
            Err(_) if sys::sim() => self.toast("Tone 440 Hz", 1500),
            Err(_) => self.toast("Tone FAILED", 2000),
        }
    }

    /// Global Play/Pause: toggle MPD. Stops a running test tone first.
    fn play_pause(&mut self) {
        if let Some(mut t) = self.tone.take() {
            proc::stop_child(&mut t, Duration::from_millis(300));
        }
        let (st, msg) = sys::mpd_toggle();
        if st != PlayState::Unknown {
            self.p.status.set_play(st);
        }
        self.toast(msg, 1400);
    }

    /// Settings > Display > Backlight test: GPIO27 low for 3 s (inferred
    /// backlight pin; if the screen does not go dark, GPIO27 is not it).
    fn backlight_test(&mut self) {
        self.busy("Backlight off 3 s");
        if let Some(l) = self.p.lcd.as_mut() {
            l.backlight(false);
            std::thread::sleep(Duration::from_secs(3));
            l.backlight(true);
        }
        self.p.fb.mark_all_dirty();
        self.toast("Back on. Did it go dark?", 2500);
    }

    /// Settings > Display > Panel SPI speed: 32 -> 48 -> 64 -> 32 MHz. The
    /// new clock is tried at once; OK within 10 s keeps it, otherwise it
    /// reverts (so an unreadable screen fixes itself).
    fn cycle_spi(&mut self) {
        let cur = spi_mhz_now();
        let ch = lcd::SPI_MHZ_CHOICES;
        let next = ch[(ch.iter().position(|m| *m == cur).unwrap_or(0) + 1) % ch.len()];
        self.apply_spi(next);
        if next == lcd::SPI_HZ / 1_000_000 {
            let _ = conf::set("display", "spi_mhz", &next.to_string());
            self.spi_trial = None;
            self.toast(format!("SPI {next} MHz (safe default)"), 1800);
        } else {
            self.spi_trial = Some((cur, Instant::now() + SPI_KEEP_WINDOW));
            self.toast(format!("SPI {next} MHz: OK = keep (10 s)"), 9500);
        }
    }

    fn apply_spi(&mut self, mhz: u32) {
        std::env::set_var("ZPOD_SPI_MHZ", mhz.to_string());
        if !self.p.is_sim() {
            self.p.release_display();
            if let Err(e) = self.p.reacquire_display() {
                eprintln!("launcher: SPI {mhz} MHz reopen failed: {e}");
            }
        }
        self.p.fb.mark_all_dirty();
        self.redraw = true;
    }

    fn spi_trial_tick(&mut self) {
        if let Some((prev, until)) = self.spi_trial {
            if Instant::now() >= until {
                self.spi_trial = None;
                self.apply_spi(prev);
                self.toast(format!("Reverted to SPI {prev} MHz"), 2000);
            }
        }
    }

    // ------------------------------------------------------------- control
    fn ctl_next(&self) -> Option<ctl::Pending> {
        self.ctl.as_ref().and_then(|r| r.try_recv().ok())
    }

    /// Index of an installed app, after a fresh scan (it may just have been pushed).
    fn app_index(&mut self, id: &str) -> Option<usize> {
        self.rescan(true);
        self.apps.iter().position(|a| a.manifest.app.id == id)
    }

    /// Control requests while no app runs (home screen or a System page).
    fn ctl_idle(&mut self) {
        use zpod_apps::ctl::{Request, Status};
        while let Some(p) = self.ctl_next() {
            match p.req.clone() {
                Request::Launch(id) | Request::Restart(Some(id)) => match self.app_index(&id) {
                    Some(_) => {
                        self.pending_launch = Some(id.clone());
                        p.reply(format!("ok launching {id}"));
                    }
                    None => p.reply(format!("err {id} is not installed (or BROKEN: zpod-app list)")),
                },
                Request::Usb(_) => ctl_usb(p, None),
                Request::Restart(None) => p.reply("err no app is running (restart <id> starts one)"),
                Request::Stop(_) | Request::Status => p.reply(Status::Idle.reply()),
                Request::Version => p.reply(version_reply()),
            }
        }
    }

    // ------------------------------------------------------------- launch
    fn launch(&mut self, idx: usize) {
        let app = self.apps[idx].clone();
        let m = &app.manifest.app;
        if let Some(mut t) = self.tone.take() {
            proc::stop_child(&mut t, Duration::from_millis(300));
        }
        // Launch card: stays on the panel until the app's first frame.
        {
            let fb = &mut self.p.fb;
            fill(fb, 0, STATUS_H, 320, (240 - STATUS_H) as u32, BG);
            icons::draw_2x(fb, &self.icons[idx].normal, 160 - 32, 64);
            text::draw_aligned(fb, Font::Title, &m.name, 160, 144, Align::Center, FG);
            text::draw_aligned(fb, Font::Small, "Hold Home 3 s to force quit", 160, 200, Align::Center, FAINT);
        }
        self.p.flush();
        self.p.screenshot(&format!("launch-{}", m.id));
        let exec = app.exec_path();
        let mut env = vec![
            ("ZPOD_APP_ID".to_string(), m.id.clone()),
            ("ZPOD_APP_DIR".to_string(), app.dir.display().to_string()),
            ("ZPOD_CORE_VERSION".to_string(), zpod_apps::core_version().to_string()),
        ];
        if self.p.is_sim() {
            // sim: give the child its own script (default: exit at once)
            let script = std::env::var(format!("ZPOD_SIM_CHILD_SCRIPT_{}", m.id.to_uppercase().replace('-', "_")))
                .unwrap_or_else(|_| "/dev/null".into());
            env.push(("ZPOD_SIM_SCRIPT".into(), script));
        }
        self.p.release_display();
        eprintln!("launcher: starting {} {} ({})", m.id, m.version, exec.display());
        // Per-app log (stdout + stderr). The sim keeps inheriting ours unless
        // ZPOD_LOG_DIR is set.
        let mut log = if self.p.is_sim() && std::env::var_os("ZPOD_LOG_DIR").is_none() {
            None
        } else {
            match zpod_apps::logs::open_for_launch(&zpod_apps::logs::log_dir(), &m.id) {
                Ok(f) => Some(f),
                Err(e) => {
                    eprintln!("launcher: {} log: {e}", m.id);
                    None
                }
            }
        };
        let started = Instant::now();
        let result = proc::spawn_app(&exec, &m.args, &app.dir, &env, self.app_user.as_ref(), log.as_ref());
        if let Some(f) = log.as_mut() {
            use std::io::Write;
            let pid = result.as_ref().map(|c| c.id().to_string()).unwrap_or_else(|e| format!("- ({e})"));
            let _ = writeln!(f, "--- {} start {} {} pid {pid}", log_time(), m.id, m.version);
        }
        let mut outcome: Option<String> = None;
        let mut ctl_stopped = false;
        match result {
            Err(e) => outcome = Some(format!("{}: {}", m.name, e.kind())),
            Ok(mut child) => {
                let mut home_down: Option<Instant> = None;
                let mut alarm_at = Instant::now();
                loop {
                    match child.try_wait() {
                        Ok(Some(st)) => {
                            if !st.success() {
                                use std::os::unix::process::ExitStatusExt;
                                outcome = Some(match (st.code(), st.signal()) {
                                    (Some(c), _) => format!("{} exited ({c})", m.name),
                                    (_, Some(s)) => format!("{} crashed (signal {s})", m.name),
                                    _ => format!("{} failed", m.name),
                                });
                            }
                            break;
                        }
                        Ok(None) => {}
                        Err(_) => break,
                    }
                    if proc::terminating() {
                        proc::stop_child(&mut child, Duration::from_secs(2));
                        break;
                    }
                    if let Some(p) = self.ctl_next() {
                        use zpod_apps::ctl::{Request, Status};
                        let running = Status::Running { id: m.id.clone(), pid: child.id(), secs: started.elapsed().as_secs() };
                        // what to start after stopping this one (None: just stop)
                        let next: Option<Option<String>> = match p.req.clone() {
                            Request::Status => {
                                p.reply(running.reply());
                                continue;
                            }
                            Request::Version => {
                                p.reply(version_reply());
                                continue;
                            }
                            Request::Usb(_) => {
                                ctl_usb(p, Some((m, child.id())));
                                continue;
                            }
                            Request::Stop(Some(x)) if x != m.id => {
                                p.reply(format!("ok {x} is not running ({} is)", m.id));
                                continue;
                            }
                            Request::Launch(x) if x == m.id => {
                                p.reply(format!("ok {x} is already running"));
                                continue;
                            }
                            Request::Stop(_) => {
                                p.reply(format!("ok stopped {}", m.id));
                                Some(None)
                            }
                            Request::Restart(None) => {
                                p.reply(format!("ok restarting {}", m.id));
                                Some(Some(m.id.clone()))
                            }
                            Request::Restart(Some(x)) | Request::Launch(x) => {
                                let (list, _) = zpod_apps::discover(&self.apps_dir);
                                if list.iter().any(|a| a.manifest.app.id == x) {
                                    p.reply(format!("ok {} {x}", if x == m.id { "restarting" } else { "launching" }));
                                    Some(Some(x))
                                } else {
                                    p.reply(format!("err {x} is not installed (or BROKEN: zpod-app list)"));
                                    None
                                }
                            }
                        };
                        if let Some(then) = next {
                            eprintln!("launcher: control socket: stopping {}", m.id);
                            proc::stop_child(&mut child, Duration::from_millis(1500));
                            ctl_stopped = true;
                            self.pending_launch = then;
                            break;
                        }
                    }
                    // In sim mode the child owns the script; don't consume it.
                    let ev = if self.p.is_sim() {
                        std::thread::sleep(Duration::from_millis(50));
                        None
                    } else {
                        self.p.next_event(Some(Duration::from_millis(100)))
                    };
                    if let Some(ev) = ev {
                        if ev.key == Key::Home {
                            match ev.state {
                                KeyState::Press => home_down = Some(Instant::now()),
                                KeyState::Release => home_down = None,
                                KeyState::Repeat => {}
                            }
                        }
                    }
                    if alarm_at.elapsed() >= Duration::from_secs(1) {
                        alarm_at = Instant::now();
                        if self.alarm_due_soon() {
                            eprintln!("launcher: alarm due, closing {}", m.id);
                            proc::stop_child(&mut child, Duration::from_millis(1500));
                            break;
                        }
                    }
                    if home_down.is_some_and(|t| t.elapsed() >= KILL_HOLD) {
                        eprintln!("launcher: Home held 3 s, stopping {}", m.id);
                        proc::stop_child(&mut child, Duration::from_millis(1500));
                        outcome = Some(format!("{} force quit", m.name));
                        break;
                    }
                }
            }
        }
        eprintln!("launcher: {} ran {:.1}s", m.id, started.elapsed().as_secs_f32());
        if let Some(f) = log.as_mut() {
            use std::io::Write;
            let how = match (&outcome, ctl_stopped) {
                (Some(o), _) => o.clone(),
                (None, true) => "stopped (control socket)".into(),
                (None, false) => "exited (0)".into(),
            };
            let _ = writeln!(f, "--- {} end {}: {how} after {:.1} s", log_time(), m.id, started.elapsed().as_secs_f32());
        }
        if let Err(e) = self.p.reacquire_display() {
            eprintln!("launcher: cannot re-open display: {e}");
            std::thread::sleep(Duration::from_millis(500));
            let _ = self.p.reacquire_display();
        }
        self.p.drain_input();
        self.p.status.volume_changed();
        self.status = StatusInfo::default(); // force status bar redraw
        self.rescan(false);
        if let Some(o) = outcome {
            self.toast(o, 2500);
        }
        self.redraw = true;
    }

}


/// Subtitle of a folder row: its own, else the first few apps in it.
fn folder_sub(t: &zpod_apps::tree::Tree, apps: &[AppInfo], n: &zpod_apps::tree::Node) -> String {
    if !n.sub.is_empty() {
        return n.sub.clone();
    }
    let names: Vec<&str> = n
        .entries
        .iter()
        .map(|e| match e {
            zpod_apps::tree::Entry::App { app, label, .. } => apps.get(*app).map(|_| label.as_str()).unwrap_or(""),
            zpod_apps::tree::Entry::Folder(f) => t.nodes[*f].title.as_str(),
        })
        .take(3)
        .collect();
    names.join(", ")
}

fn it_is_spi(items: &[Item], sel: usize) -> bool {
    items.get(sel) == Some(&Item::SpiSpeed)
}

fn signal_colour(dbm: f32, connected: bool) -> Rgb565 {
    if connected {
        return CYAN;
    }
    match tools::bars(dbm) {
        4 | 3 => OK,
        2 => ACCENT,
        1 => WARN,
        _ => FAINT,
    }
}

/// Full-screen alarm / timer ring.
fn draw_ring(fb: &mut zpod_ui::Framebuffer, r: alarm::Ring, st: &StatusInfo) {
    let timer = r == alarm::Ring::Timer;
    draw_status_bar(fb, if timer { "Timer" } else { "Alarm" }, st);
    let area = rect(0, LIST_Y, 320, LIST_H as u32);
    let _ = fb.fill_solid(&area, BG);
    let t = sys::now_local();
    let blink = t.second % 2 == 0;
    text::draw_aligned(fb, Font::Title, if timer { "Time's up" } else { "Alarm" }, 160, LIST_Y + 16, Align::Center, if blink { ACCENT } else { WARN });
    let now = format!("{:02}:{:02}", t.hour, t.minute);
    text::draw_aligned(fb, Font::Digits58, &now, 160, LIST_Y + 56, Align::Center, FG);
    if timer {
        text::draw_aligned(fb, Font::Body, "Any button stops the timer", 160, LIST_Y + 140, Align::Center, DIM);
        widgets::hints(fb, &[("any key", "stop")]);
    } else {
        let a = alarm::load();
        let l = format!("OK = stop     other buttons = snooze {} min", a.snooze_min);
        text::draw_aligned(fb, Font::Small, &l, 160, LIST_Y + 140, Align::Center, DIM);
        widgets::hints(fb, &[("OK", "stop"), ("other", "snooze"), ("Vol", "volume")]);
    }
}

fn draw_transfer(fb: &mut zpod_ui::Framebuffer, x: &transfer::Info) {
    let area = rect(0, LIST_Y, 320, LIST_H as u32);
    let _ = fb.fill_solid(&area, BG);
    let mut y = LIST_Y + 4;
    let kv = |fb: &mut zpod_ui::Framebuffer, y: &mut i32, k: &str, v: &str, c: Rgb565| {
        text::draw(fb, Font::Small, k, 10, *y + 3, DIM);
        text::draw_fit(fb, Font::Body, v, 66, *y, 246, c);
        *y += 18;
    };
    let usb = match &x.usb {
        Some(ip) => format!("{ip}   (ssh, scp, rsync)"),
        None => "not connected".into(),
    };
    kv(fb, &mut y, "USB", &usb, if x.usb.is_some() { FG } else { DIM });
    let wifi = match &x.wlan {
        Some(ip) => format!("{ip}   {}.local", x.host),
        None => "not connected".into(),
    };
    kv(fb, &mut y, "Wi-Fi", &wifi, if x.wlan.is_some() { FG } else { DIM });
    let web = if x.drop_on {
        format!("http://{}:{DROP_PORT}/  (drop box)", x.usb.clone().or(x.wlan.clone()).unwrap_or_else(|| "<ip>".into()))
    } else {
        "drop box off (Wi-Fi tools > Drop box)".into()
    };
    kv(fb, &mut y, "Browser", &web, if x.drop_on { FG } else { DIM });
    let rs = match x.rsync {
        Some(true) => format!("ON  rsync://{}/music/  (USB only)", x.usb.clone().unwrap_or_else(|| "10.12.194.1".into())),
        Some(false) => "off   (Square = on; USB only)".into(),
        None => "not installed".into(),
    };
    kv(fb, &mut y, "rsync", &rs, if x.rsync == Some(true) { WARN } else { DIM });
    let free = x
        .free
        .map(|u| format!("{} free of {}{}", storage::human(u.avail), storage::human(u.total), if x.on_data { "  (/data)" } else { "" }))
        .unwrap_or_else(|| "n/a".into());
    kv(fb, &mut y, "Space", &free, if x.free.map(|u| u.pct() >= 90).unwrap_or(false) { WARN } else { FG });
    let lib = if !x.mpd_ok {
        ("MPD not running".to_string(), WARN)
    } else if x.updating {
        ("rescanning...".to_string(), ACCENT)
    } else {
        let when = x.db_update.map(|t| transfer::when(t, &sys::now_local())).unwrap_or_else(|| "?".into());
        (format!("{} songs, scanned {when}", x.songs.map(|n| n.to_string()).unwrap_or_else(|| "?".into())), FG)
    };
    kv(fb, &mut y, "Library", &lib.0, lib.1);
    y += 2;
    text::draw(fb, Font::Small, "Newest files", 10, y, ACCENT);
    y += 14;
    if x.recent.is_empty() {
        text::draw(fb, Font::Small, "(none yet; copy music into music/)", 14, y, FAINT);
    }
    for f in x.recent.iter().take(4) {
        text::draw_fit(fb, Font::Small, f, 14, y, 296, FG);
        y += 13;
    }
    widgets::hints(fb, &[("OK", "rescan"), ("Sq", "rsync share"), ("Lt/X", "back")]);
}

enum Action {
    Launch(usize),
    PlayPause,
    Tone,
    Backlight,
    CycleSpi,
    Power(StopKind),
}

fn drop_running() -> bool {
    if sys::sim() {
        return std::env::var_os("ZPOD_SIM_DROP").is_some();
    }
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], DROP_PORT));
    std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(150)).is_ok()
}

// ------------------------------------------------------------- end screens

fn final_frame(fb: &mut zpod_ui::Framebuffer, kind: StopKind) {
    fill(fb, 0, 0, 320, 240, BLACK);
    match kind {
        StopKind::Reboot => {
            text::draw_aligned(fb, Font::Huge, "Rebooting...", 160, 100, Align::Center, WHITE);
        }
        StopKind::Poweroff => {
            let green = zpod_ui::theme::rgb(0, 200, 0);
            for i in 0..3 {
                let _ = rect(6 + i, 6 + i, (308 - 2 * i) as u32, (228 - 2 * i) as u32)
                    .into_styled(PrimitiveStyle::with_stroke(green, 1))
                    .draw(fb);
            }
            text::draw(fb, Font::Huge, "ZPOD off", 24, 40, WHITE);
            text::draw(fb, Font::Title, "Safe to power off", 24, 108, zpod_ui::theme::rgb(0, 230, 0));
            text::draw(fb, Font::Body, "Slide the power switch off.", 24, 160, zpod_ui::theme::rgb(200, 200, 200));
            let t = sys::now_local();
            let s = format!("Halted {:04}-{:02}-{:02} {:02}:{:02}", t.year, t.month, t.day, t.hour, t.minute);
            text::draw(fb, Font::Body, &s, 24, 186, zpod_ui::theme::rgb(150, 150, 150));
        }
    }
}

/// Leave the panel in a clear state (same rules as M3 zpod_ui.end_screen).
fn end_screen(p: &mut Platform, kind: Option<StopKind>) {
    let conf = std::fs::read_to_string("/etc/zpod/ui.conf").unwrap_or_default();
    let mode = proc::shutdown_screen_mode(&conf);
    if p.lcd.is_none() && !p.is_sim() {
        let _ = p.reacquire_display();
    }
    match (kind, mode) {
        (Some(StopKind::Poweroff), "message") | (Some(StopKind::Reboot), _) => {
            final_frame(&mut p.fb, kind.unwrap());
            p.fb.mark_all_dirty();
            p.flush();
            p.screenshot(match kind {
                Some(StopKind::Reboot) => "end-reboot",
                _ => "end-poweroff",
            });
        }
        _ => {
            if let Some(l) = p.lcd.as_mut() {
                l.blank_off();
            }
            fill(&mut p.fb, 0, 0, 320, 240, BLACK);
            p.flush();
            p.screenshot("end-off");
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    proc::install_signal_handlers();
    if args.first().map(String::as_str) == Some("--end-screen") {
        let kind = match args.get(1).map(String::as_str) {
            Some("poweroff") | Some("halt") => Some(StopKind::Poweroff),
            Some("reboot") => Some(StopKind::Reboot),
            _ => None,
        };
        match Platform::open() {
            Ok(mut p) => end_screen(&mut p, kind),
            Err(e) => eprintln!("zpod-launcher: display: {e}"),
        }
        return;
    }
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("usage: zpod-launcher [--end-screen poweroff|reboot|off]");
        return;
    }

    set_data_dir();
    proc::stop_splash();
    let _ = std::fs::remove_file(proc::STOP_KIND);
    let p = loop {
        match Platform::open() {
            Ok(p) => break p,
            Err(e) => {
                eprintln!("zpod-launcher: display: {e}; retrying");
                if proc::terminating() {
                    return;
                }
                std::thread::sleep(Duration::from_secs(2));
            }
        }
    };
    let mut l = Launcher::new(p);
    l.p.fb.mark_all_dirty();
    let mut status_redraw_at = Instant::now();
    let mut powering = false;
    let mut first_frame_logged = false;

    loop {
        if proc::terminating() {
            break;
        }
        l.ctl_idle();
        if let Some(id) = l.pending_launch.take() {
            if let Some(i) = l.app_index(&id) {
                l.launch(i);
                continue;
            }
            l.toast(format!("{id} is not installed"), 2500);
        }
        // status bar (cheap: StatusSource rate-limits each source)
        let st = l.p.status.poll();
        if st != l.status {
            l.status = st;
            l.redraw = true;
        }
        if let Some((_, until)) = &l.toast {
            if Instant::now() >= *until {
                l.toast = None;
                l.redraw = true;
            }
        }
        if let Some(t) = l.tone.as_mut() {
            if let Ok(Some(_)) = t.try_wait() {
                l.tone = None;
                l.redraw = true;
            }
        }
        l.spi_trial_tick();
        if l.tools.poll() {
            l.redraw = true;
            let page = l.menus.page();
            let n = l.rows_for(page);
            if matches!(page, Page::WifiScan | Page::BtScan) && l.menus.list().sel >= n.0.max(1) {
                l.menus.list_mut().sel = 0;
                l.menus.list_mut().top = 0;
            }
        }
        l.check_alarm();
        if let Some(r) = &l.ringer {
            if r.expired() {
                l.end_ring(false);
            } else if Instant::now() >= status_redraw_at {
                l.redraw = true;
                status_redraw_at = Instant::now() + Duration::from_secs(1);
            }
        }
        l.tone_tick();
        if l.wt.poll_status() {
            l.redraw = true;
        }
        if l.menus.page() == Page::WifiPassword {
            l.pw_poll();
        } else if l.pw.is_some() && !l.menus.stack.iter().any(|(p, _)| *p == Page::WifiPassword) {
            l.pw = None; // left (e.g. Home from the USB confirmation): wipe the field
        }
        if let Some(msg) = l.wt.poll_job() {
            l.p.status.net_changed();
            if let (Page::WifiPassword, Some(pw)) = (l.menus.page(), l.pw.as_mut()) {
                pw.note = Some((msg.starts_with("Joined"), msg.trim_start_matches("Failed: ").to_string()));
                l.redraw = true;
            } else {
                l.toast(msg, 2200);
            }
            l.wifi = sys::wifi_info();
            l.refresh_server();
        }
        let page = l.menus.page();
        if page == Page::WifiSignal && l.wt.meter_tick() {
            l.redraw = true;
        }
        if matches!(page, Page::Wifi | Page::WifiMode | Page::HotspotDevices) && !l.wt.busy() {
            l.wt.refresh_status(false); // every 5 s, on a thread
        }
        l.usb_job_poll();
        if matches!(page, Page::Usb | Page::UsbConfirm(_)) && l.usb_job.is_some() && Instant::now() >= status_redraw_at {
            l.redraw = true; // "Switching USB... N s"
            status_redraw_at = Instant::now() + Duration::from_secs(1);
        }
        if page == Page::Usb && l.usb_job.is_none() && l.usb_at.elapsed() >= Duration::from_secs(3) {
            l.refresh_usb();
        }
        if page == Page::Battery && l.batt_at.elapsed() >= Duration::from_secs(20) {
            l.load_battery();
        }
        if matches!(page, Page::Hotspot | Page::Server(_)) && l.srv_at.elapsed() >= Duration::from_secs(5) && !l.wt.busy() {
            l.wt.refresh_hotspot();
            l.refresh_server();
        }
        if page == Page::Transfer && l.xfer_at.elapsed() >= Duration::from_secs(if l.xfer.updating { 1 } else { 3 }) {
            l.refresh_transfer();
        }
        if matches!(page, Page::WifiScan | Page::BtScan | Page::WifiChannels | Page::WifiDevices) && l.tools.busy() && Instant::now() >= status_redraw_at {
            l.redraw = true; // "N s ago" / "scanning" hint
            status_redraw_at = Instant::now() + Duration::from_secs(1);
        }
        let live = matches!(page, Page::Status | Page::About | Page::DateTime);
        if live && page != Page::DateTime && Instant::now() >= status_redraw_at {
            l.redraw = true;
            status_redraw_at = Instant::now() + Duration::from_secs(1);
        }
        if page == Page::Menu(MenuId::Top) {
            l.rescan(false);
        }
        if l.redraw && !powering {
            l.draw();
        }
        l.p.flush();
        if !first_frame_logged && !l.p.is_sim() {
            first_frame_logged = true;
            proc::boot_mark("launcher-first-frame");
        }

        let mut timeout = match (&l.toast, live) {
            (Some((_, until)), _) => until.saturating_duration_since(Instant::now()).max(Duration::from_millis(20)),
            (None, true) => Duration::from_millis(1000 - sys::subsec_ms().min(999) + 5),
            _ => Duration::from_secs(1),
        };
        if let Some((_, until)) = l.spi_trial {
            timeout = timeout.min(until.saturating_duration_since(Instant::now()).max(Duration::from_millis(20)));
        }
        if l.tools.busy() || l.wt.busy() || page == Page::WifiSignal || l.usb_job.is_some() {
            timeout = timeout.min(Duration::from_millis(250));
        }
        if l.ctl.is_some() {
            timeout = timeout.min(Duration::from_millis(250)); // control socket latency
        }
        if page == Page::WifiPassword {
            timeout = timeout.min(Duration::from_millis(30)); // keyboard events are polled
        }
        let Some(ev) = l.p.next_event(Some(timeout)) else {
            if l.p.is_sim() && sim_done() {
                break;
            }
            continue;
        };
        if powering {
            continue;
        }
        match l.key(ev) {
            Some(Action::Launch(i)) => l.launch(i),
            Some(Action::PlayPause) => l.play_pause(),
            Some(Action::Tone) => l.toggle_tone(),
            Some(Action::Backlight) => l.backlight_test(),
            Some(Action::CycleSpi) => l.cycle_spi(),
            Some(Action::Power(kind)) => {
                powering = true;
                l.toast = None;
                l.busy(if kind == StopKind::Reboot { "Rebooting..." } else { "Shutting down" });
                if l.p.is_sim() {
                    end_screen(&mut l.p, Some(kind));
                    return;
                }
                proc::power(kind);
                // SIGTERM follows from init; keep the Busy screen until then.
            }
            None => {}
        }
    }
    if let Some(mut t) = l.tone.take() {
        proc::stop_child(&mut t, Duration::from_millis(200));
    }
    if let Some(mut r) = l.ringer.take() {
        r.stop();
    }
    if proc::restart_requested() {
        // A new launcher binary takes over: keep the panel as it is.
        eprintln!("zpod-launcher: stopping (restart)");
        let _ = std::fs::remove_file(zpod_apps::ctl::socket_path());
        return;
    }
    let kind = proc::stop_kind();
    eprintln!("zpod-launcher: stopping ({kind:?})");
    end_screen(&mut l.p, kind);
}

/// `usb ...` on the control or app socket. Apps (app socket): only the app
/// on the screen (`front`: its manifest + pid; the caller may be a child),
/// and only with `permissions = ["usb-host"]`. The switch runs on a thread
/// that answers when done, so the app's wait loop keeps running.
fn ctl_usb(p: ctl::Pending, front: Option<(&zpod_apps::manifest::AppSection, u32)>) {
    use zpod_apps::ctl::{Request, UsbReq};
    let Request::Usb(u) = p.req.clone() else {
        p.reply("err not a usb request");
        return;
    };
    match u {
        UsbReq::Status => {
            let i = usbrole::status();
            p.reply(format!("ok {}", i.to_words()));
        }
        UsbReq::Set { to, force, persist } => {
            if p.from_app {
                let Some((m, pid)) = front else {
                    p.reply("err only the app on the screen can switch USB");
                    return;
                };
                let caller = match p.peer_pid {
                    Some(pp) => usbrole::is_descendant(pp, pid),
                    None => sys::sim(),
                };
                if !caller {
                    p.reply("err only the app on the screen can switch USB");
                    return;
                }
                if !m.has_permission("usb-host") {
                    p.reply(format!("err {} lacks permission usb-host (manifest permissions)", m.id));
                    return;
                }
                eprintln!("launcher: {} asks for usb {}{}", m.id, to.word(), if force { " (forced)" } else { "" });
            }
            std::thread::spawn(move || {
                let r = usbrole::set(to, force, persist);
                p.reply(match r {
                    Ok(x) => format!("ok {x}"),
                    Err(e) => format!("err {e}"),
                });
            });
        }
    }
}

fn version_reply() -> String {
    format!("ok zpod-launcher {} ctl {}", env!("CARGO_PKG_VERSION"), zpod_apps::ctl::PROTOCOL)
}

/// Local time for per-app log lines.
fn log_time() -> String {
    let t = sys::now_local();
    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", t.year, t.month, t.day, t.hour, t.minute, t.second)
}

/// Apps and the alarm share one data dir. The unit sets ZPOD_DATA_DIR
/// (/data/zpod/appdata on M5 images); otherwise use the app user's default
/// so root (the launcher) and the apps agree. Children inherit it.
fn set_data_dir() {
    if sys::sim() {
        return;
    }
    if std::env::var_os("ZPOD_DATA_DIR").is_some_and(|d| std::path::Path::new(&d).is_dir()) {
        return;
    }
    let d = if std::path::Path::new("/data/zpod/appdata").is_dir() { "/data/zpod/appdata" } else { "/var/lib/zpod/home/.local/share/zpod" };
    std::env::set_var("ZPOD_DATA_DIR", d);
}

/// The headless simulator exits the process itself when its script ends;
/// nothing to do here, but keep the hook for clarity.
fn sim_done() -> bool {
    false
}
