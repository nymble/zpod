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
//! * `zpod-launcher --end-screen poweroff|reboot|off` draws just that frame
//!   (for BusyBox shutdown scripts).
//! * M6: Battery page (history from zpod-battery), About split from it,
//!   Wi-Fi tools (signal meter, channels, saved networks, hotspot), Drop box
//!   and Book catalog switches ([`server`]), and a first-screen notice when
//!   bootfs setup files were applied after the first boot.

mod icons;
mod menu;
mod pages;
mod proc;
mod ring;
mod server;
mod tools;
mod transfer;
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

struct Launcher {
    p: Platform,
    menus: Menus,
    apps: Vec<AppInfo>,
    broken: Vec<(String, String)>,
    icons: Vec<icons::Icon>,
    system_icon: icons::Icon,
    apps_dir: PathBuf,
    apps_stamp: Option<std::time::SystemTime>,
    status: StatusInfo,
    gauge: sys::Gauge,
    toast: Option<(String, Instant)>,
    tone: Option<Child>,
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
    batt: Option<pages::BattView>,
    batt_at: Instant,
    srv: server::State,
    srv_at: Instant,
    notice: Vec<String>,
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
            icons: Vec::new(),
            system_icon: icons::from_png(icons::SYSTEM_PNG).unwrap(),
            apps_dir,
            apps_stamp: None,
            status: StatusInfo::default(),
            gauge: sys::Gauge::default(),
            toast: None,
            tone: None,
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
            batt: None,
            batt_at: Instant::now(),
            srv: server::State::default(),
            srv_at: Instant::now(),
            notice: Vec::new(),
        };
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
        self.icons = apps.iter().map(|a| icons::load(a.icon_path().as_deref())).collect();
        self.apps = apps;
        self.broken = broken;
        let n = menu::items(MenuId::Top, self.apps.len()).len();
        if let Some((_, ls)) = self.menus.stack.first_mut() {
            ls.scroll_into_view(n, (LIST_H / HOME_ROW) as usize);
        }
        self.redraw = true;
    }

    fn toast(&mut self, msg: impl Into<String>, ms: u64) {
        self.toast = Some((msg.into(), Instant::now() + Duration::from_millis(ms)));
        self.redraw = true;
    }

    fn rows_for(&self, page: Page) -> (usize, usize) {
        match page {
            Page::Menu(MenuId::Top) => (0, (LIST_H / HOME_ROW) as usize),
            Page::Menu(_) => (0, (LIST_H / SUB_ROW) as usize),
            Page::Apps => (self.apps.len() + self.broken.len(), (LIST_H / APPS_ROW) as usize),
            Page::WifiScan => (self.tools.wifi.items.len(), (LIST_H / APPS_ROW) as usize),
            Page::BtScan => (self.tools.bt.items.len(), (LIST_H / APPS_ROW) as usize),
            Page::WifiSaved => (self.wt.saved.len(), (LIST_H / APPS_ROW) as usize),
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
            Page::WifiScan if !self.tools.wifi.items.is_empty() => format!("Wi-Fi ({})", self.tools.wifi.items.len()),
            Page::BtScan if !self.tools.bt.items.is_empty() => format!("Bluetooth ({})", self.tools.bt.items.len()),
            Page::ScanDetail => self.tools.detail_title.clone(),
            _ => menu::title(page).to_string(),
        };
        let fb = &mut self.p.fb;
        draw_status_bar(fb, &title, &st);
        let area = rect(0, LIST_Y, 320, LIST_H as u32);
        let n_apps = self.apps.len();
        match page {
            Page::Menu(id) => {
                let items = menu::items(id, n_apps);
                let ls = *self.menus.list();
                let sel = ls.sel;
                let (apps, icons, sysi) = (&self.apps, &self.icons, &self.system_icon);
                let top = id == MenuId::Top;
                let row_h = if top { HOME_ROW } else { SUB_ROW };
                widgets::draw_list(fb, area, row_h, &ls, items.len(), |i| {
                    let it = items[i];
                    let (t, sub) = menu::label(it);
                    let mut r = match it {
                        Item::App(a) => {
                            let m = &apps[a].manifest.app;
                            let ic = if i == sel { &icons[a].selected } else { &icons[a].normal };
                            Row { title: &m.name, sub: Some(&m.description), right: None, icon: Some(ic), mark: None }
                        }
                        _ => {
                            let ic = if i == sel { &sysi.selected } else { &sysi.normal };
                            Row { title: t, sub: if sub.is_empty() { None } else { Some(sub) }, right: None, icon: if top { Some(ic) } else { None }, mark: None }
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
                    if !top && sub.is_empty() {
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
            Page::WifiChannels => {
                let chans = wifi::congestion(&self.tools.wifi.items);
                let conn = self.tools.wifi.items.iter().find(|n| n.associated).map(|n| n.channel);
                let age = self.tools.wifi.age_text();
                pages::draw_channels(fb, &chans, conn, self.tools.wifi.busy.is_some(), &age, self.tools.wifi.items.len());
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
                widgets::hints(fb, &[("OK", "switch to"), ("Sq", "refresh"), ("Lt/X", "back")]);
            }
            Page::Hotspot => {
                let home = self.wt.home_ssid.clone().or_else(|| self.wifi.ssid.clone());
                pages::draw_hotspot(fb, self.wt.hotspot.as_ref(), self.wt.job.as_ref().map(|j| j.what.as_str()), home.as_deref(), &self.srv);
            }
            Page::Server(opds) => {
                let hs = self.wt.hotspot.as_ref().is_some_and(|h| h.active);
                pages::draw_server(fb, opds, &self.srv, hs);
            }
            Page::Notice => pages::draw_notice(fb, &self.notice),
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
                let _ = fb.fill_solid(&area, BG);
                let w = &self.wifi;
                let rows = [
                    ("SSID", w.ssid.clone().unwrap_or_else(|| "--".into())),
                    ("Signal", w.signal.as_ref().map(|s| format!("{s}%")).unwrap_or_else(|| "--".into())),
                    ("IP", w.ip.clone().unwrap_or_else(|| "--".into())),
                    ("MAC", w.mac.clone().unwrap_or_else(|| "--".into())),
                    ("Pwr save", w.powersave.clone().unwrap_or_else(|| "?".into())),
                    ("usb0", sys::ipv4("usb0").unwrap_or_else(|| "--".into())),
                ];
                for (i, (k, v)) in rows.iter().enumerate() {
                    let y = LIST_Y + 8 + i as i32 * 30;
                    text::draw(fb, Font::Small, k, 12, y + 4, DIM);
                    text::draw_fit(fb, Font::BodyBold, v, 96, y, 216, FG);
                }
                widgets::hints(fb, &[("OK", "refresh"), ("Lt/X", "back"), ("Home", "top")]);
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
                widgets::hints(fb, &[("OK", "details"), ("Sq", if t.wifi.busy.is_some() { "scanning" } else { "rescan" }), ("Tri", &sort), ("", &age)]);
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
                let (apps, broken) = (&self.apps, &self.broken);
                let mut subs: Vec<String> = Vec::with_capacity(n);
                for a in apps {
                    subs.push(format!("{}  \u{b7}  {}", a.manifest.app.id, a.manifest.app.description));
                }
                for (_, why) in broken {
                    subs.push(why.clone());
                }
                let ls = *self.menus.list();
                widgets::draw_list(fb, area, APPS_ROW, &ls, n, |i| {
                    if i < apps.len() {
                        let a = &apps[i].manifest.app;
                        Row { title: &a.name, sub: Some(&subs[i]), right: Some(&a.version), icon: None, mark: None }
                    } else {
                        let (id, _) = &broken[i - apps.len()];
                        Row { title: id, sub: Some(&subs[i]), right: Some("BROKEN"), icon: None, mark: Some(WARN) }
                    }
                });
                widgets::hints(fb, &[("Lt/X", "back"), ("", "install: sudo zpod-app install x.zpa")]);
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
            (Page::WifiSaved, Nav::Other(Key::Square)) => {
                self.wt.refresh_saved();
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
        let eff = self.menus.handle(nav, self.apps.len(), rows);
        self.redraw = true;
        match eff {
            Effect::None | Effect::Redraw => None,
            Effect::Launch(i) => Some(Action::Launch(i)),
            Effect::PlayPause => Some(Action::PlayPause),
            Effect::Tone => Some(Action::Tone),
            Effect::BacklightTest => Some(Action::Backlight),
            Effect::CycleSpi => Some(Action::CycleSpi),
            Effect::RefreshWifi => {
                self.wifi = sys::wifi_info();
                None
            }
            Effect::Activate(p) => {
                match p {
                    Page::WifiSaved => {
                        let sel = self.menus.list().sel;
                        if let Some(m) = self.wt.connect(sel) {
                            self.toast(m, 1200);
                        }
                    }
                    Page::Hotspot => {
                        let best = (!self.tools.wifi.items.is_empty()).then(|| wifi::best_channel(&wifi::congestion(&self.tools.wifi.items)));
                        self.wt.toggle_hotspot(best);
                    }
                    Page::Server(opds) => match server::toggle(opds) {
                        Ok(st) => {
                            let on = if opds { st.opds } else { st.dropbox };
                            self.srv = st;
                            self.toast(format!("{} {}", if opds { "Book catalog" } else { "Drop box" }, if on { "on" } else { "off" }), 1500);
                        }
                        Err(e) => self.toast(e, 2500),
                    },
                    Page::Notice => self.dismiss_notice(),
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
                match p {
                    Page::Wifi => self.wifi = sys::wifi_info(),
                    Page::Battery => self.load_battery(),
                    Page::WifiSignal => {
                        let sel = self.menus.list().sel;
                        let t = self.tools.wifi.items.get(sel).filter(|n| !n.associated).map(|n| wifitools::Target { bssid: n.bssid.clone(), ssid: n.ssid.clone(), channel: n.channel });
                        self.wt.open_meter(t);
                    }
                    Page::WifiChannels => self.tools.wifi.start_if_stale(tools::wifi_job),
                    Page::WifiSaved => self.wt.refresh_saved(),
                    Page::Hotspot => {
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
                None
            }
            Effect::Power(reboot) => Some(Action::Power(if reboot { StopKind::Reboot } else { StopKind::Poweroff })),
        }
    }

    fn toggle_tone(&mut self) {
        if let Some(mut c) = self.tone.take() {
            proc::stop_child(&mut c, Duration::from_millis(300));
            self.toast("Tone stopped", 1500);
            return;
        }
        // The DAC is exclusive: pause MPD first.
        if let Ok(mut c) = zpod_mpd::Client::connect_default() {
            let _ = c.simple("pause 1");
            self.p.status.set_play(PlayState::Paused);
        }
        let mut cmd = std::process::Command::new("timeout");
        cmd.args(["10", "speaker-test", "-D", "default", "-t", "sine", "-f", "440", "-c", "2"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
        match if sys::sim() { Err(std::io::Error::other("sim")) } else { cmd.spawn() } {
            Ok(c) => {
                self.tone = Some(c);
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
        let started = Instant::now();
        let result = proc::spawn_app(&exec, &m.args, &app.dir, &env, self.app_user.as_ref());
        let mut outcome: Option<String> = None;
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
        "drop box off (Tools > Drop box)".into()
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
        if let Some(msg) = l.wt.poll_job() {
            l.toast(msg, 2200);
            l.wifi = sys::wifi_info();
            l.refresh_server();
        }
        let page = l.menus.page();
        if page == Page::WifiSignal && l.wt.meter_tick() {
            l.redraw = true;
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
        if matches!(page, Page::WifiScan | Page::BtScan | Page::WifiChannels) && l.tools.busy() && Instant::now() >= status_redraw_at {
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
        if l.tools.busy() || l.wt.busy() || page == Page::WifiSignal {
            timeout = timeout.min(Duration::from_millis(250));
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
    let kind = proc::stop_kind();
    eprintln!("zpod-launcher: stopping ({kind:?})");
    end_screen(&mut l.p, kind);
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
