//! USB controller role of the Pi's data port, for honest UI states
//! ("USB is the Mac link right now" instead of "no device"), and the app
//! side of switching it: [`info`] / [`request`] talk to the launcher's app
//! socket (`/run/zpod/app.sock`, protocol in `zpod_apps::ctl`). The
//! launcher only switches for the app on the screen whose manifest lists
//! `permissions = ["usb-host"]`; the app must ask the user first
//! ([`prompt`] has the shared wording).

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UsbRole {
    /// dwc2 dr_mode=peripheral: CDC-NCM gadget to the computer, no host.
    Gadget,
    /// dr_mode=host: keyboards, mice, GPS can be plugged in.
    Host,
    /// dr_mode=otg: decided by the cable (ID pin); treat as maybe-host.
    Otg,
    Unknown,
}

impl UsbRole {
    pub fn parse(s: &str) -> UsbRole {
        match s.trim_matches(|c: char| c == '\0' || c.is_whitespace()) {
            "peripheral" | "gadget" | "device" => UsbRole::Gadget,
            "host" => UsbRole::Host,
            "otg" => UsbRole::Otg,
            _ => UsbRole::Unknown,
        }
    }

    pub fn can_host(self) -> bool {
        matches!(self, UsbRole::Host | UsbRole::Otg)
    }

    pub fn label(self) -> &'static str {
        match self {
            UsbRole::Gadget => "gadget (computer link)",
            UsbRole::Host => "host",
            UsbRole::Otg => "OTG",
            UsbRole::Unknown => "unknown",
        }
    }

    /// One line for screens that need host mode.
    pub fn host_hint(self) -> &'static str {
        match self {
            UsbRole::Gadget => "USB is the computer link now (gadget mode)",
            UsbRole::Host => "USB host mode",
            UsbRole::Otg => "USB OTG: host with an OTG adapter",
            UsbRole::Unknown => "USB role unknown",
        }
    }
}

/// What the launcher reports (`usb status`): the role now, the boot role,
/// whether leaving the computer link is safe (Wi-Fi address + SSH).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RoleInfo {
    pub role: Option<UsbRole>,
    /// role on the boot partition (what a reboot restores)
    pub boot: Option<UsbRole>,
    pub next: Option<UsbRole>,
    /// switched live (RAM only)
    pub live: bool,
    pub wifi_ip: Option<String>,
    pub ssh: bool,
    pub safe: bool,
    pub can_switch: bool,
    /// "file" (zpod-usb-role.txt), "config" (config.txt dwc2 line), "none"
    pub persist: String,
    /// computer link state ("configured", "not attached", "none")
    pub link: Option<String>,
    /// "067b:2303 Prolific ..." per plugged-in USB device (not hubs)
    pub devices: Vec<String>,
}

impl RoleInfo {
    /// `key=value` words or lines (`zpod-usb-role status`, `usb status`).
    pub fn parse(text: &str) -> RoleInfo {
        let mut i = RoleInfo::default();
        let words: Vec<&str> = if text.contains('\n') { text.lines().collect() } else { text.split(' ').collect() };
        for w in words {
            let Some((k, v)) = w.trim_end().split_once('=') else { continue };
            let some = |v: &str| (!v.is_empty()).then(|| v.to_string());
            match k {
                "role" => i.role = Some(UsbRole::parse(v)).filter(|r| *r != UsbRole::Unknown),
                "boot" => i.boot = Some(UsbRole::parse(v)).filter(|r| *r != UsbRole::Unknown),
                "next" => i.next = Some(UsbRole::parse(v)).filter(|r| *r != UsbRole::Unknown),
                "live" => i.live = v == "1",
                "wifi_ip" => i.wifi_ip = some(v),
                "ssh" => i.ssh = v == "1",
                "safe" => i.safe = v == "1",
                "can_switch" => i.can_switch = v == "1",
                "persist" => i.persist = v.to_string(),
                "link" => i.link = some(&v.replace('_', " ")).filter(|s| s != "none"),
                "dev" => i.devices.push(v.replace('_', " ")),
                _ => {}
            }
        }
        i
    }

    /// One-line form for the app socket (spaces in device names -> '_').
    pub fn to_words(&self) -> String {
        let r = |o: Option<UsbRole>| match o {
            Some(UsbRole::Gadget) => "gadget",
            Some(UsbRole::Host) => "host",
            Some(UsbRole::Otg) => "otg",
            _ => "unknown",
        };
        let b = |x: bool| if x { "1" } else { "0" };
        let mut s = format!(
            "role={} boot={} next={} live={} wifi_ip={} ssh={} safe={} can_switch={} persist={} link={}",
            r(self.role),
            r(self.boot),
            r(self.next),
            b(self.live),
            self.wifi_ip.as_deref().unwrap_or(""),
            b(self.ssh),
            b(self.safe),
            b(self.can_switch),
            if self.persist.is_empty() { "none" } else { &self.persist },
            self.link.as_deref().unwrap_or("none").replace(' ', "_"),
        );
        for d in &self.devices {
            s.push_str(&format!(" dev={}", d.replace(' ', "_")));
        }
        s
    }
}

fn app_socket() -> PathBuf {
    std::env::var_os("ZPOD_APP_SOCKET").map(Into::into).unwrap_or_else(|| "/run/zpod/app.sock".into())
}

fn ask(line: &str, timeout: Duration) -> Result<String, String> {
    if crate::role::sim_switch() {
        return sim_ask(line);
    }
    let path = app_socket();
    let mut s = std::os::unix::net::UnixStream::connect(&path).map_err(|e| format!("launcher app socket {}: {e} (launcher too old?)", path.display()))?;
    s.set_read_timeout(Some(timeout)).ok();
    s.set_write_timeout(Some(timeout)).ok();
    writeln!(s, "{line}").map_err(|e| e.to_string())?;
    let mut r = String::new();
    BufReader::new(s).take(4096).read_line(&mut r).map_err(|e| format!("no reply from launcher: {e}"))?;
    let r = r.trim();
    if let Some(x) = r.strip_prefix("ok") {
        Ok(x.trim().to_string())
    } else if let Some(x) = r.strip_prefix("err") {
        Err(x.trim().to_string())
    } else {
        Err(format!("bad reply from launcher: '{r}'"))
    }
}

/// `ZPOD_SIM` + `ZPOD_SIM_USB_SWITCH=ok|<error text>`: answer in-process
/// (simulator screenshots; flips `ZPOD_USB_ROLE`).
fn sim_switch() -> bool {
    std::env::var_os("ZPOD_SIM").is_some() && std::env::var_os("ZPOD_SIM_USB_SWITCH").is_some()
}

/// Simulator view of the USB state: role from `ZPOD_USB_ROLE`, Wi-Fi from
/// `ZPOD_SIM_IP_WLAN0` (unset = no Wi-Fi), `ZPOD_SIM_USB_NEXT=host`.
pub fn sim_info() -> RoleInfo {
    let role = usb_role();
    let ip = std::env::var("ZPOD_SIM_IP_WLAN0").ok().filter(|s| !s.is_empty());
    let next = if std::env::var("ZPOD_SIM_USB_NEXT").as_deref() == Ok("host") { UsbRole::Host } else { UsbRole::Gadget };
    RoleInfo {
        role: Some(role),
        boot: Some(next),
        next: Some(next),
        live: role != next,
        ssh: ip.is_some(),
        safe: ip.is_some(),
        wifi_ip: ip,
        can_switch: true,
        persist: "file".into(),
        link: (role != UsbRole::Host).then(|| "configured".into()),
        devices: if role == UsbRole::Gadget { vec![] } else { vec!["067b:2303 Prolific USB-Serial Controller".into(), "046d:c52b Logitech USB Receiver".into()] },
    }
}

fn sim_ask(line: &str) -> Result<String, String> {
    if line == "usb status" {
        return Ok(sim_info().to_words());
    }
    let v = std::env::var("ZPOD_SIM_USB_SWITCH").unwrap_or_default();
    if v != "ok" {
        return Err(v);
    }
    let to = line.split_whitespace().nth(1).unwrap_or("");
    let r = match to {
        "host" => "host",
        "auto" => "otg",
        _ => "gadget",
    };
    std::env::set_var("ZPOD_USB_ROLE", r);
    Ok(r.to_string())
}

/// Ask the launcher for the USB state.
pub fn info() -> Result<RoleInfo, String> {
    ask("usb status", Duration::from_secs(3)).map(|r| RoleInfo::parse(&r))
}

/// Switch now (the user already confirmed). `force`: even without Wi-Fi
/// SSH. Returns the new role word. Takes up to ~8 s.
pub fn request(host: bool, force: bool) -> Result<String, String> {
    let line = format!("usb {}{}", if host { "host" } else { "gadget" }, if force { " force" } else { "" });
    ask(&line, Duration::from_secs(15))
}

/// Shared wording of the "Switch USB?" dialog (apps and Tools > USB).
pub mod prompt {
    use super::RoleInfo;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Tone {
        Normal,
        Good,
        Warn,
        Dim,
    }

    pub struct Prompt {
        pub title: &'static str,
        pub lines: Vec<(String, Tone)>,
        /// OK may switch (false: only "switch anyway" after the warning)
        pub ok_switches: bool,
    }

    /// Leaving the computer link for USB devices.
    /// `for_what`: the app asking ("GPS"), None from Tools > USB.
    pub fn to_host(i: &RoleInfo, for_what: Option<&str>) -> Prompt {
        let first = match for_what {
            Some(w) => format!("{w} needs the USB port for devices."),
            None => "Keyboard, mouse, GPS: OTG adapter or hub.".to_string(),
        };
        let mut lines = vec![(first, Tone::Normal), ("The USB link to the computer stops now.".to_string(), Tone::Normal)];
        match (&i.wifi_ip, i.safe) {
            (Some(ip), true) => lines.push((format!("Wi-Fi SSH stays: root@{ip}"), Tone::Good)),
            (Some(ip), false) => lines.push((format!("Wi-Fi {ip}, but SSH is not listening"), Tone::Warn)),
            (None, _) => lines.push(("No Wi-Fi: only this screen can switch back".to_string(), Tone::Warn)),
        }
        lines.push(("Back: Tools > USB, or restart the ZPOD".to_string(), Tone::Dim));
        Prompt { title: "Switch USB to devices?", lines, ok_switches: i.safe }
    }

    /// Back to the computer link.
    pub fn to_gadget(i: &RoleInfo) -> Prompt {
        let n = i.devices.len();
        let mut lines = vec![
            (if n > 0 { format!("{n} USB device(s) disconnect now.") } else { "USB devices disconnect now.".to_string() }, Tone::Normal),
            ("The computer link is back in about 5 s.".to_string(), Tone::Normal),
        ];
        if i.next == Some(super::UsbRole::Host) {
            lines.push(("Next boot: still USB devices".to_string(), Tone::Dim));
        }
        Prompt { title: "Connect to computer?", lines, ok_switches: true }
    }

    /// OTG: the cable decides (ID pin).
    pub fn to_auto(i: &RoleInfo) -> Prompt {
        let mut lines = vec![
            ("Computer cable: computer link.".to_string(), Tone::Normal),
            ("OTG adapter / hub: USB devices.".to_string(), Tone::Normal),
            ("Experimental: needs the ID pin; untested here.".to_string(), Tone::Dim),
        ];
        match (&i.wifi_ip, i.safe) {
            (Some(ip), true) => lines.push((format!("Wi-Fi SSH stays: root@{ip}"), Tone::Good)),
            _ => lines.push(("No Wi-Fi SSH: keep the computer cable in".to_string(), Tone::Warn)),
        }
        Prompt { title: "Auto: follow the cable?", lines, ok_switches: true }
    }
}

/// `ZPOD_USB_ROLE` (simulator, tests), else `/run/zpod/usb-role` (written by

/// The app side of "Switch USB?": ask the launcher for the role info, show
/// [`prompt`], and run the switch on a thread. No graphics or keys here:
/// the app draws `prompt()` / `busy()` and maps its keys to [`Answer`].
pub mod ask {
    use super::{info, prompt, request, RoleInfo, UsbRole};
    use std::sync::mpsc::{channel, Receiver};
    use std::time::Instant;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Answer {
        /// OK: switch (only when Wi-Fi SSH is safe, or going back to the computer)
        Yes,
        /// Triangle: switch to devices even without Wi-Fi SSH
        Force,
        Cancel,
    }

    #[derive(Debug, PartialEq, Eq)]
    pub enum Outcome {
        Nothing,
        Cancelled,
        Switched(UsbRole),
        Failed(String),
        /// Yes on an unsafe switch: show this hint
        NeedForce,
    }

    enum Phase {
        Idle,
        Asking { host: bool, info: RoleInfo },
        Switching { host: bool, since: Instant, rx: Receiver<Result<String, String>> },
    }

    pub struct UsbAsk {
        app: String,
        phase: Phase,
    }

    impl UsbAsk {
        pub fn new(app: &str) -> Self {
            UsbAsk { app: app.to_string(), phase: Phase::Idle }
        }
        /// Ask the launcher for the role info and open the dialog. Err: no
        /// app socket (launcher too old) or the tool is missing.
        pub fn open(&mut self, host: bool) -> Result<(), String> {
            let i = info()?;
            if !i.can_switch {
                return Err("this image cannot switch USB live".into());
            }
            self.open_with(host, i);
            Ok(())
        }
        pub fn open_with(&mut self, host: bool, info: RoleInfo) {
            self.phase = Phase::Asking { host, info };
        }
        pub fn active(&self) -> bool {
            !matches!(self.phase, Phase::Idle)
        }
        pub fn prompt(&self) -> Option<prompt::Prompt> {
            match &self.phase {
                Phase::Asking { host: true, info } => Some(prompt::to_host(info, Some(&self.app))),
                Phase::Asking { host: false, info } => Some(prompt::to_gadget(info)),
                _ => None,
            }
        }
        /// "Switching USB... 2 s" while the switch runs.
        pub fn busy(&self) -> Option<String> {
            match &self.phase {
                Phase::Switching { host, since, .. } => Some(format!("{} {} s", if *host { "Switching USB to devices..." } else { "Connecting to computer..." }, since.elapsed().as_secs())),
                _ => None,
            }
        }
        /// (key word, action) pairs for the hint bar: "OK"/"Tri" + "X".
        pub fn hints(&self) -> Vec<(&'static str, &'static str)> {
            match self.prompt() {
                Some(p) if p.ok_switches => vec![("OK", "switch"), ("X", "cancel")],
                Some(_) => vec![("Tri", "switch anyway"), ("X", "cancel")],
                None => vec![],
            }
        }
        pub fn answer(&mut self, a: Answer) -> Outcome {
            let Phase::Asking { host, info } = &self.phase else { return Outcome::Nothing };
            let host = *host;
            let ok = prompt::to_host(info, None).ok_switches || !host;
            let force = match a {
                Answer::Cancel => {
                    self.phase = Phase::Idle;
                    return Outcome::Cancelled;
                }
                Answer::Yes if !ok => return Outcome::NeedForce,
                Answer::Yes => false,
                Answer::Force if host && !ok => true,
                Answer::Force => return Outcome::Nothing,
            };
            let (tx, rx) = channel();
            std::thread::spawn(move || {
                let _ = tx.send(request(host, force));
            });
            self.phase = Phase::Switching { host, since: Instant::now(), rx };
            Outcome::Nothing
        }
        /// Call every loop; the switch result once.
        pub fn poll(&mut self) -> Outcome {
            let Phase::Switching { rx, .. } = &self.phase else { return Outcome::Nothing };
            let r = match rx.try_recv() {
                Ok(r) => r,
                Err(std::sync::mpsc::TryRecvError::Empty) => return Outcome::Nothing,
                Err(_) => Err("switch thread died".into()),
            };
            self.phase = Phase::Idle;
            match r {
                Ok(w) => Outcome::Switched(UsbRole::parse(w.split_whitespace().next().unwrap_or(""))),
                Err(e) => Outcome::Failed(e),
            }
        }
    }
}

/// S35usbgadget), else the device tree's dwc2 `dr_mode`.
pub fn usb_role() -> UsbRole {
    if let Ok(v) = std::env::var("ZPOD_USB_ROLE") {
        return UsbRole::parse(&v);
    }
    if let Ok(v) = std::fs::read_to_string("/run/zpod/usb-role") {
        let r = UsbRole::parse(&v);
        if r != UsbRole::Unknown {
            return r;
        }
    }
    dt_role(Path::new("/proc/device-tree/soc"))
}

/// dr_mode of the first `usb@*` node under `soc` (the Zero has one).
pub fn dt_role(soc: &Path) -> UsbRole {
    let Ok(rd) = std::fs::read_dir(soc) else { return UsbRole::Unknown };
    let mut nodes: Vec<_> = rd.flatten().filter(|e| e.file_name().to_string_lossy().starts_with("usb@")).collect();
    nodes.sort_by_key(|e| e.file_name());
    for n in nodes {
        if let Ok(b) = std::fs::read(n.path().join("dr_mode")) {
            return UsbRole::parse(&String::from_utf8_lossy(&b));
        }
    }
    UsbRole::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_roles() {
        assert_eq!(UsbRole::parse("peripheral\0"), UsbRole::Gadget);
        assert_eq!(UsbRole::parse("host\n"), UsbRole::Host);
        assert_eq!(UsbRole::parse("otg"), UsbRole::Otg);
        assert_eq!(UsbRole::parse(""), UsbRole::Unknown);
        assert!(UsbRole::Host.can_host() && !UsbRole::Gadget.can_host());
    }

    #[test]
    fn role_info_round_trip() {
        let kv = "role=gadget\nboot=gadget\nlive=0\nnext=host\npersist=file\nwifi_ip=192.0.2.7\nssh=1\nsafe=1\nlink=not attached\ncan_switch=1\ndev=067b:2303 Prolific USB-Serial\n";
        let i = RoleInfo::parse(kv);
        assert_eq!(i.role, Some(UsbRole::Gadget));
        assert_eq!(i.next, Some(UsbRole::Host));
        assert!(i.safe && i.ssh && i.can_switch && !i.live);
        assert_eq!(i.wifi_ip.as_deref(), Some("192.0.2.7"));
        assert_eq!(i.link.as_deref(), Some("not attached"));
        assert_eq!(i.devices, vec!["067b:2303 Prolific USB-Serial".to_string()]);
        let j = RoleInfo::parse(&i.to_words());
        assert_eq!(j.link.as_deref(), Some("not attached"));
        assert_eq!(j, i);
        let none = RoleInfo::parse("role=host wifi_ip= ssh=0 safe=0 link=none");
        assert_eq!(none.wifi_ip, None);
        assert_eq!(none.link, None);
        assert!(!none.safe);
    }

    #[test]
    fn prompts_warn_without_wifi() {
        let mut i = RoleInfo::parse("role=gadget safe=1 ssh=1 wifi_ip=192.0.2.7");
        let p = prompt::to_host(&i, Some("GPS"));
        assert!(p.ok_switches);
        assert!(p.lines[0].0.starts_with("GPS needs"));
        assert!(p.lines.iter().any(|(l, t)| l.contains("root@192.0.2.7") && *t == prompt::Tone::Good));
        i.safe = false;
        i.wifi_ip = None;
        let p = prompt::to_host(&i, None);
        assert!(!p.ok_switches);
        assert!(p.lines.iter().any(|(_, t)| *t == prompt::Tone::Warn));
        assert!(prompt::to_gadget(&i).ok_switches);
    }

    #[test]
    fn app_socket_client() {
        use std::os::unix::net::UnixListener;
        let path = std::env::temp_dir().join(format!("zpod-input-app-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let l = UnixListener::bind(&path).unwrap();
        let t = std::thread::spawn(move || {
            let mut got = Vec::new();
            for _ in 0..2 {
                let (s, _) = l.accept().unwrap();
                let mut line = String::new();
                BufReader::new(&s).read_line(&mut line).unwrap();
                let reply = if line.trim() == "usb status" { "ok role=gadget safe=0 wifi_ip=".to_string() } else { "err usb-host permission missing".to_string() };
                got.push(line.trim().to_string());
                (&s).write_all(format!("{reply}\n").as_bytes()).unwrap();
            }
            got
        });
        std::env::set_var("ZPOD_APP_SOCKET", &path);
        assert_eq!(info().unwrap().role, Some(UsbRole::Gadget));
        assert_eq!(request(true, true).unwrap_err(), "usb-host permission missing");
        std::env::remove_var("ZPOD_APP_SOCKET");
        assert_eq!(t.join().unwrap(), vec!["usb status".to_string(), "usb host force".to_string()]);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn device_tree_fixture() {
        let root = std::env::temp_dir().join(format!("zpod-input-dt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("usb@7e980000")).unwrap();
        std::fs::write(root.join("usb@7e980000/dr_mode"), b"peripheral\0").unwrap();
        assert_eq!(dt_role(&root), UsbRole::Gadget);
        std::fs::write(root.join("usb@7e980000/dr_mode"), b"host\0").unwrap();
        assert_eq!(dt_role(&root), UsbRole::Host);
        assert_eq!(dt_role(&root.join("missing")), UsbRole::Unknown);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn ask_dialog_flow() {
        use super::ask::{Answer, Outcome, UsbAsk};
        let unsafe_i = RoleInfo { role: Some(UsbRole::Gadget), can_switch: true, ..Default::default() };
        let mut a = UsbAsk::new("GPS");
        assert!(!a.active() && a.prompt().is_none());
        a.open_with(true, unsafe_i.clone());
        assert!(a.active());
        let p = a.prompt().unwrap();
        assert_eq!(p.title, "Switch USB to devices?");
        assert!(p.lines[0].0.starts_with("GPS needs"));
        assert_eq!(a.hints()[0], ("Tri", "switch anyway"));
        assert_eq!(a.answer(Answer::Yes), Outcome::NeedForce);
        assert!(a.active());
        assert_eq!(a.answer(Answer::Cancel), Outcome::Cancelled);
        assert!(!a.active());
        // back to the computer never needs Wi-Fi
        a.open_with(false, RoleInfo { role: Some(UsbRole::Host), ..unsafe_i });
        assert_eq!(a.prompt().unwrap().title, "Connect to computer?");
        assert_eq!(a.hints()[0], ("OK", "switch"));
        assert_eq!(a.answer(Answer::Force), Outcome::Nothing);
        assert!(a.busy().is_none());
    }
}
