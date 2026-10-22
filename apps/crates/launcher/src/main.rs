//! ZPOD launcher: the one LCD owner between apps.
//!
//! * Home: installed apps (from /opt/zpod/apps/*/manifest.toml) + System.
//! * System: everything the M3 Python zpod-ui did (Status, test tone,
//!   Volume, Wi-Fi info, Reboot, Shutdown) plus File transfer and Apps.
//! * Launching: draw a launch card, release spidev/GPIO, run the app as the
//!   unprivileged app user in its own process group, wait. Holding Home for
//!   3 s kills a hung app. Afterwards re-acquire the panel (warm, no flash).
//! * SIGTERM: M3 end screen ("ZPOD off / Safe to power off", "Rebooting...",
//!   or blank+backlight off) chosen by /etc/zpod/ui.conf [shutdown] screen=.
//! * `zpod-launcher --end-screen poweroff|reboot|off` draws just that frame
//!   (for BusyBox shutdown scripts).

mod icons;
mod proc;

use std::path::PathBuf;
use std::process::Child;
use std::time::{Duration, Instant};

use embedded_graphics::{pixelcolor::Rgb565, prelude::*, primitives::*};
use proc::StopKind;
use zpod_apps::AppInfo;
use zpod_core::{Event, Key, KeyState, PlayState, StatusInfo};
use zpod_hal::{sys, Platform};
use zpod_ui::{
    fill, rect,
    status::draw_status_bar,
    text::{self, Align},
    theme::*,
    widgets::{self, ListState, Row, HINTS_H},
    Font,
};

const LIST_Y: i32 = STATUS_H;
const LIST_H: i32 = 240 - STATUS_H - HINTS_H;
const HOME_ROW: i32 = 40;
const SYS_ROW: i32 = 25;
const KILL_HOLD: Duration = Duration::from_secs(3);
const DROP_PORT: u16 = 8080;
const MUSIC_DIR: &str = "/srv/zpod/media/music";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SysItem {
    Status,
    Volume,
    Wifi,
    Transfer,
    Apps,
    Tone,
    Reboot,
    Shutdown,
}

const SYS_ITEMS: [(SysItem, &str); 8] = [
    (SysItem::Status, "Status"),
    (SysItem::Volume, "Volume"),
    (SysItem::Wifi, "Wi-Fi info"),
    (SysItem::Transfer, "File transfer"),
    (SysItem::Apps, "Installed apps"),
    (SysItem::Tone, "Play test tone"),
    (SysItem::Reboot, "Reboot"),
    (SysItem::Shutdown, "Shutdown"),
];

#[derive(Clone, PartialEq, Debug)]
enum Screen {
    Home,
    System,
    Status,
    Volume,
    Wifi(sys::WifiInfo),
    Transfer,
    Apps,
    Confirm(StopKind),
    Busy(String),
}

struct Launcher {
    p: Platform,
    screen: Screen,
    home: ListState,
    sys: ListState,
    apps_list: ListState,
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
}

fn dir_stamp(p: &std::path::Path) -> Option<std::time::SystemTime> {
    let a = std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let b = std::fs::metadata("/run/zpod/apps-changed").and_then(|m| m.modified()).ok();
    a.max(b)
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
            screen: Screen::Home,
            home: ListState::default(),
            sys: ListState::default(),
            apps_list: ListState::default(),
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
        };
        l.rescan(true);
        l
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
        self.home.scroll_into_view(self.apps.len() + 1, (LIST_H / HOME_ROW) as usize);
        self.redraw = true;
    }

    fn toast(&mut self, msg: impl Into<String>, ms: u64) {
        self.toast = Some((msg.into(), Instant::now() + Duration::from_millis(ms)));
        self.redraw = true;
    }

    // ---------------------------------------------------------------- draw
    fn title(&self) -> &'static str {
        match self.screen {
            Screen::Home => "ZPOD",
            Screen::System => "System",
            Screen::Status => "Status",
            Screen::Volume => "Volume",
            Screen::Wifi(_) => "Wi-Fi",
            Screen::Transfer => "File transfer",
            Screen::Apps => "Apps",
            Screen::Confirm(_) | Screen::Busy(_) => "ZPOD",
        }
    }

    fn draw(&mut self) {
        let st = self.status.clone();
        let title = self.title();
        let fb = &mut self.p.fb;
        draw_status_bar(fb, title, &st);
        let area = rect(0, LIST_Y, 320, LIST_H as u32);
        match &self.screen {
            Screen::Home => {
                let n = self.apps.len() + 1;
                let sel = self.home.sel;
                let (apps, icons, sysi) = (&self.apps, &self.icons, &self.system_icon);
                widgets::draw_list(fb, area, HOME_ROW, &self.home, n, |i| {
                    if i < apps.len() {
                        let a = &apps[i].manifest.app;
                        let ic = if i == sel { &icons[i].selected } else { &icons[i].normal };
                        Row { title: &a.name, sub: Some(&a.description), right: None, icon: Some(ic), mark: None }
                    } else {
                        let ic = if i == sel { &sysi.selected } else { &sysi.normal };
                        Row { title: "System", sub: Some("Status, volume, Wi-Fi, transfer, power"), right: None, icon: Some(ic), mark: None }
                    }
                });
                widgets::hints(fb, &[("OK", "open"), ("Tri", "clock"), ("Play", "music"), ("Vol", "volume")]);
            }
            Screen::System => {
                widgets::draw_list(fb, area, SYS_ROW, &self.sys, SYS_ITEMS.len(), |i| {
                    let (it, name) = SYS_ITEMS[i];
                    let mut r = Row::simple(name);
                    if it == SysItem::Tone && self.tone.is_some() {
                        r.right = Some("playing");
                    }
                    if matches!(it, SysItem::Reboot | SysItem::Shutdown) {
                        r.mark = Some(WARN);
                    }
                    r
                });
                widgets::hints(fb, &[("OK", "select"), ("X", "back"), ("Play", "tone/music")]);
            }
            Screen::Status => {
                let _ = fb.fill_solid(&area, BG);
                let t = sys::now_local();
                let batt = self.gauge.read();
                let mpd = match sys::mpd_play_state() {
                    PlayState::Playing => "playing",
                    PlayState::Paused => "paused",
                    PlayState::Stopped => "stopped",
                    PlayState::Unknown => "not running",
                };
                let rows: [(&str, String); 9] = [
                    ("Time", format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", t.year, t.month, t.day, t.hour, t.minute, t.second)),
                    ("wlan0", sys::ipv4("wlan0").unwrap_or_else(|| "--".into())),
                    ("usb0", sys::ipv4("usb0").unwrap_or_else(|| "--".into())),
                    ("Battery", batt.map(|(p, v, c)| format!("{p:.0}%  {v:.2} V{}", if c { "  charging" } else { "" })).unwrap_or_else(|| "n/a".into())),
                    ("Volume", st.volume_pct.map(|v| format!("{v}%")).unwrap_or_else(|| "n/a".into())),
                    ("DAC", if sys::card_present() || sys::sim() { "ok".into() } else { "missing".into() }),
                    ("Music", mpd.into()),
                    ("Uptime", sys::uptime()),
                    ("Core", format!("{}  ({} apps)", zpod_apps::core_version(), self.apps.len())),
                ];
                for (i, (k, v)) in rows.iter().enumerate() {
                    let y = LIST_Y + 4 + i as i32 * 22;
                    text::draw(fb, Font::Small, k, 10, y + 3, DIM);
                    text::draw_fit(fb, Font::Body, v, 84, y, 230, if v == "missing" { WARN } else { FG });
                }
                widgets::hints(fb, &[("X", "back"), ("Home", "menu")]);
            }
            Screen::Volume => {
                let _ = fb.fill_solid(&area, BG);
                let v = st.volume_pct;
                let s = v.map(|v| format!("{v}%")).unwrap_or_else(|| "n/a".into());
                text::draw_aligned(fb, Font::Digits58, &s.replace('%', ""), 160, 48, Align::Center, ACCENT);
                if v.is_some() {
                    text::draw(fb, Font::Title, "%", 160 + text::width(Font::Digits58, &s.replace('%', "")) / 2 + 4, 62, ACCENT);
                }
                let _ = RoundedRectangle::with_equal_corners(rect(20, 130, 280, 26), Size::new(6, 6))
                    .into_styled(PrimitiveStyle::with_stroke(FG, 2))
                    .draw(fb);
                if let Some(v) = v {
                    let w = (274 * v as u32 / 100).max(1);
                    let _ = RoundedRectangle::with_equal_corners(rect(23, 133, w, 20), Size::new(4, 4))
                        .into_styled(PrimitiveStyle::with_fill(OK))
                        .draw(fb);
                }
                text::draw_aligned(fb, Font::Small, "ALSA 'Digital' on the PCM5122 (also: Vol+/- keys)", 160, 172, Align::Center, DIM);
                widgets::hints(fb, &[("Up/Rt", "+3%"), ("Dn/Lt", "-3%"), ("X", "back")]);
            }
            Screen::Wifi(w) => {
                let _ = fb.fill_solid(&area, BG);
                let rows = [
                    ("SSID", w.ssid.clone().unwrap_or_else(|| "--".into())),
                    ("Signal", w.signal.as_ref().map(|s| format!("{s}%")).unwrap_or_else(|| "--".into())),
                    ("IP", w.ip.clone().unwrap_or_else(|| "--".into())),
                    ("Pwr save", w.powersave.clone().unwrap_or_else(|| "?".into())),
                    ("usb0", sys::ipv4("usb0").unwrap_or_else(|| "--".into())),
                ];
                for (i, (k, v)) in rows.iter().enumerate() {
                    let y = LIST_Y + 10 + i as i32 * 30;
                    text::draw(fb, Font::Small, k, 12, y + 4, DIM);
                    text::draw_fit(fb, Font::BodyBold, v, 96, y, 216, FG);
                }
                widgets::hints(fb, &[("OK", "refresh"), ("X", "back")]);
            }
            Screen::Transfer => {
                let _ = fb.fill_solid(&area, BG);
                let usb = sys::ipv4("usb0");
                let wlan = sys::ipv4("wlan0");
                let drop_on = drop_running();
                let mut lines: Vec<(Font, String, Rgb565, i32)> = Vec::new();
                lines.push((Font::BodyBold, "USB cable (fastest)".into(), ACCENT, 18));
                match &usb {
                    Some(ip) => {
                        lines.push((Font::Body, format!("rsync / scp:  pi@{ip}"), FG, 17));
                        if drop_on {
                            lines.push((Font::Body, format!("browser:  http://{ip}:{DROP_PORT}/"), FG, 17));
                        }
                    }
                    None => lines.push((Font::Body, "not connected".into(), DIM, 17)),
                }
                lines.push((Font::BodyBold, "Wi-Fi  (or zpod.local)".into(), ACCENT, 18));
                match &wlan {
                    Some(ip) => {
                        lines.push((Font::Body, format!("rsync / scp:  pi@{ip}"), FG, 17));
                        if drop_on {
                            lines.push((Font::Body, format!("browser:  http://{ip}:{DROP_PORT}/ +token"), FG, 17));
                        }
                    }
                    None => lines.push((Font::Body, "not connected".into(), DIM, 17)),
                }
                lines.push((Font::Small, format!("Music goes to {MUSIC_DIR}"), DIM, 14));
                lines.push((Font::Small, "Mac: scripts/zpod-push-music.sh ~/Music/Album".into(), DIM, 14));
                lines.push(if drop_on {
                    (Font::Small, "Drop server: running".into(), OK, 14)
                } else {
                    (Font::Small, "Drop server: off (sudo systemctl start zpod-drop.socket)".into(), DIM, 14)
                });
                let mut y = LIST_Y + 6;
                for (f, s, c, dy) in &lines {
                    if *f == Font::BodyBold && y > LIST_Y + 6 {
                        y += 4;
                    }
                    text::draw_fit(fb, *f, s, 12, y, 300, *c);
                    y += dy;
                }
                widgets::hints(fb, &[("X", "back"), ("Home", "menu")]);
            }
            Screen::Apps => {
                let n = self.apps.len() + self.broken.len();
                let (apps, broken) = (&self.apps, &self.broken);
                let mut subs: Vec<String> = Vec::with_capacity(n);
                for a in apps {
                    subs.push(format!("{}  \u{b7}  {}", a.manifest.app.id, a.manifest.app.description));
                }
                for (_, why) in broken {
                    subs.push(why.clone());
                }
                widgets::draw_list(fb, area, 34, &self.apps_list, n, |i| {
                    if i < apps.len() {
                        let a = &apps[i].manifest.app;
                        Row { title: &a.name, sub: Some(&subs[i]), right: Some(&a.version), icon: None, mark: None }
                    } else {
                        let (id, _) = &broken[i - apps.len()];
                        Row { title: id, sub: Some(&subs[i]), right: Some("BROKEN"), icon: None, mark: Some(WARN) }
                    }
                });
                widgets::hints(fb, &[("X", "back"), ("ssh", "sudo zpod-app install x.zpa")]);
            }
            Screen::Confirm(kind) => {
                let (t, l1) = match kind {
                    StopKind::Reboot => ("Reboot?", "The ZPOD restarts (about 40 s)."),
                    StopKind::Poweroff => ("Shut down?", "Wait for \"Safe to power off\"."),
                };
                let _ = fb.fill_solid(&area, BG);
                widgets::dialog(fb, t, &[l1, "OK = yes      X = cancel"], WARN);
                widgets::hints(fb, &[("OK", "yes"), ("X", "cancel")]);
            }
            Screen::Busy(msg) => {
                let _ = fb.fill_solid(&area, BG);
                text::draw_aligned(fb, Font::Huge, msg, 160, 100, Align::Center, ACCENT);
                fill(fb, 0, 240 - HINTS_H, 320, HINTS_H as u32, BAR);
            }
        }
        if let Some((msg, _)) = &self.toast {
            widgets::toast(fb, msg);
        }
        self.redraw = false;
    }

    // -------------------------------------------------------------- input
    fn back(&mut self) {
        self.screen = match self.screen {
            Screen::Home => Screen::Home,
            Screen::System | Screen::Busy(_) => Screen::Home,
            _ => Screen::System,
        };
        self.redraw = true;
    }

    fn key(&mut self, ev: Event) -> Option<Action> {
        if !ev.is_down() {
            return None;
        }
        let k = ev.key;
        if matches!(k, Key::VolUp | Key::VolDown) {
            // zpod-buttons already changed the ALSA volume; just show it.
            std::thread::sleep(Duration::from_millis(60));
            let v = sys::volume_get();
            self.p.status.set_volume(v);
            self.status.volume_pct = v;
            self.toast(v.map(|v| format!("Vol {v}%")).unwrap_or_else(|| "Vol n/a".into()), 1500);
            return None;
        }
        if ev.state != KeyState::Press && !matches!(k, Key::Up | Key::Down | Key::Left | Key::Right) {
            return None; // autorepeat only for navigation
        }
        if matches!(self.screen, Screen::Busy(_)) {
            return None;
        }
        if let Screen::Confirm(kind) = self.screen {
            match k {
                Key::Ok | Key::Circle => return Some(Action::Power(kind)),
                Key::Cross | Key::Left | Key::Home | Key::Square => self.screen = Screen::System,
                _ => {}
            }
            self.redraw = true;
            return None;
        }
        match k {
            Key::PlayPause => return Some(Action::PlayPause),
            Key::Triangle => {
                if self.apps.iter().any(|a| a.manifest.app.id == "clock") {
                    return Some(Action::LaunchId("clock".into()));
                }
            }
            Key::Home => {
                self.screen = Screen::Home;
                self.redraw = true;
                return None;
            }
            _ => {}
        }
        let vis_home = (LIST_H / HOME_ROW) as usize;
        let vis_sys = (LIST_H / SYS_ROW) as usize;
        let is_ok = matches!(k, Key::Ok | Key::Circle | Key::Right);
        let is_back = matches!(k, Key::Cross | Key::Left | Key::Square);
        self.redraw = true;
        match self.screen.clone() {
            Screen::Home => match k {
                Key::Up => self.home.step(-1, self.apps.len() + 1, vis_home),
                Key::Down => self.home.step(1, self.apps.len() + 1, vis_home),
                _ if is_ok && ev.state == KeyState::Press => {
                    if self.home.sel < self.apps.len() {
                        return Some(Action::Launch(self.home.sel));
                    }
                    self.screen = Screen::System;
                }
                _ => {}
            },
            Screen::System => match k {
                Key::Up => self.sys.step(-1, SYS_ITEMS.len(), vis_sys),
                Key::Down => self.sys.step(1, SYS_ITEMS.len(), vis_sys),
                _ if is_back => self.back(),
                _ if is_ok && ev.state == KeyState::Press => match SYS_ITEMS[self.sys.sel].0 {
                    SysItem::Status => self.screen = Screen::Status,
                    SysItem::Volume => {
                        self.p.status.volume_changed();
                        self.screen = Screen::Volume;
                    }
                    SysItem::Wifi => self.screen = Screen::Wifi(sys::wifi_info()),
                    SysItem::Transfer => self.screen = Screen::Transfer,
                    SysItem::Apps => {
                        self.rescan(true);
                        self.screen = Screen::Apps;
                    }
                    SysItem::Tone => return Some(Action::Tone),
                    SysItem::Reboot => self.screen = Screen::Confirm(StopKind::Reboot),
                    SysItem::Shutdown => self.screen = Screen::Confirm(StopKind::Poweroff),
                },
                _ => {}
            },
            Screen::Volume => match k {
                Key::Up | Key::Right => self.set_vol(3),
                Key::Down | Key::Left => self.set_vol(-3),
                Key::Cross | Key::Ok | Key::Square => self.back(),
                _ => {}
            },
            Screen::Wifi(_) => match k {
                Key::Ok | Key::Circle => self.screen = Screen::Wifi(sys::wifi_info()),
                _ if is_back => self.back(),
                _ => {}
            },
            Screen::Apps => {
                let n = self.apps.len() + self.broken.len();
                match k {
                    Key::Up => self.apps_list.step(-1, n, (LIST_H / 34) as usize),
                    Key::Down => self.apps_list.step(1, n, (LIST_H / 34) as usize),
                    _ if is_back || k == Key::Ok => self.back(),
                    _ => {}
                }
            }
            _ => {
                if is_back || k == Key::Ok {
                    self.back();
                }
            }
        }
        None
    }

    fn set_vol(&mut self, d: i32) {
        let v = sys::volume_step(d);
        self.p.status.set_volume(v);
        self.status.volume_pct = v;
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
                self.toast("Tone 440 Hz", 1500);
            }
            Err(_) if sys::sim() => self.toast("Tone 440 Hz", 1500),
            Err(_) => self.toast("Tone FAILED", 2000),
        }
    }

    /// Play/Pause on the menus: toggle MPD if it has a queue, else the M3 test tone.
    fn play_pause(&mut self) {
        if let Ok(mut c) = zpod_mpd::Client::connect_default() {
            if let Ok(s) = c.status() {
                if s.playlist_len > 0 {
                    let (cmd, st, msg) = match s.state.as_str() {
                        "play" => ("pause 1", PlayState::Paused, "Paused"),
                        "pause" => ("pause 0", PlayState::Playing, "Playing"),
                        _ => ("play", PlayState::Playing, "Playing"),
                    };
                    let _ = c.simple(cmd);
                    self.p.status.set_play(st);
                    self.toast(msg, 1200);
                    return;
                }
            }
        }
        self.toggle_tone();
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

    fn launch_id(&mut self, id: &str) {
        if let Some(i) = self.apps.iter().position(|a| a.manifest.app.id == id) {
            self.launch(i);
        }
    }
}

enum Action {
    Launch(usize),
    LaunchId(String),
    PlayPause,
    Tone,
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
        if l.screen == Screen::Status && Instant::now() >= status_redraw_at {
            l.redraw = true;
            status_redraw_at = Instant::now() + Duration::from_secs(1);
        }
        if l.screen == Screen::Home {
            l.rescan(false);
        }
        if l.redraw {
            l.draw();
        }
        l.p.flush();

        let timeout = match (&l.screen, &l.toast) {
            (_, Some((_, until))) => until.saturating_duration_since(Instant::now()).max(Duration::from_millis(20)),
            (Screen::Status, _) => Duration::from_millis(1000 - sys::subsec_ms().min(999) + 5),
            _ => Duration::from_secs(1),
        };
        let Some(ev) = l.p.next_event(Some(timeout)) else {
            if l.p.is_sim() && sim_done() {
                break;
            }
            continue;
        };
        match l.key(ev) {
            Some(Action::Launch(i)) => l.launch(i),
            Some(Action::LaunchId(id)) => l.launch_id(&id),
            Some(Action::PlayPause) => l.play_pause(),
            Some(Action::Tone) => l.toggle_tone(),
            Some(Action::Power(kind)) => {
                l.screen = Screen::Busy(if kind == StopKind::Reboot { "Rebooting...".into() } else { "Shutting down".into() });
                l.draw();
                l.p.flush();
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
    let kind = proc::stop_kind();
    eprintln!("zpod-launcher: stopping ({kind:?})");
    end_screen(&mut l.p, kind);
}

/// The headless simulator exits the process itself when its script ends;
/// nothing to do here, but keep the hook for clarity.
fn sim_done() -> bool {
    false
}
