//! Tools > Updates: whether core updates pushed to /data (newer launcher,
//! zpod-app, buttons daemon, zpod-usb-role) load at boot, which core is
//! running, and how the last start went. The work is done by the root
//! shell tool `zpod-core-overlay` (buildroot/board/zpod/rootfs-overlay/
//! usr/sbin, docs/BOOT-UPDATES.md); this reads `status --kv` and runs
//! `enable` / `disable`. The switch takes effect at the next start.
//! Simulator: `ZPOD_CORE_SIM=<file>` with the same key=value lines.

use std::process::Command;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Info {
    /// "on" / "off" (boot option)
    pub boot_load: bool,
    /// the image applies updates at boot (false on test3)
    pub boot_support: bool,
    /// "update" (from /data) or "image"
    pub active: String,
    pub image: String,
    pub update: String,
    /// ok | fallback | skipped | off | none | pending | manual
    pub state: String,
    pub reason: String,
    /// read failed: why
    pub error: Option<String>,
}

pub fn parse(text: &str) -> Info {
    let mut i = Info::default();
    for l in text.lines() {
        let Some((k, v)) = l.split_once('=') else { continue };
        let v = v.trim().to_string();
        match k.trim() {
            "boot_load" => i.boot_load = v == "on",
            "boot_support" => i.boot_support = v == "1",
            "active" => i.active = v,
            "image" => i.image = v,
            "update" => i.update = v,
            "state" => i.state = v,
            "reason" => i.reason = v,
            _ => {}
        }
    }
    i
}

fn tool() -> Option<&'static str> {
    ["/usr/sbin/zpod-core-overlay", "/data/zpod/core/zpod-core-overlay"].into_iter().find(|p| std::path::Path::new(p).exists())
}

pub fn read() -> Info {
    if let Some(p) = std::env::var_os("ZPOD_CORE_SIM") {
        return std::fs::read_to_string(p).map(|t| parse(&t)).unwrap_or_default();
    }
    let Some(t) = tool() else { return Info { error: Some("this image has no zpod-core-overlay".into()), ..Default::default() } };
    match Command::new("/bin/sh").args([t, "status", "--kv"]).output() {
        Ok(o) => parse(&String::from_utf8_lossy(&o.stdout)),
        Err(e) => Info { error: Some(e.to_string()), ..Default::default() },
    }
}

/// Turn loading at boot on or off. Returns the toast text.
pub fn set(on: bool) -> Result<String, String> {
    let msg = if on { "Updates load at the next start" } else { "Next start uses the image core" };
    if let Some(p) = std::env::var_os("ZPOD_CORE_SIM") {
        let t = std::fs::read_to_string(&p).unwrap_or_default();
        let t: String = t.lines().filter(|l| !l.starts_with("boot_load=")).map(|l| format!("{l}\n")).collect();
        let _ = std::fs::write(&p, format!("boot_load={}\n{t}", if on { "on" } else { "off" }));
        return Ok(msg.into());
    }
    let t = tool().ok_or("this image has no zpod-core-overlay")?;
    let o = Command::new("/bin/sh").args([t, if on { "enable" } else { "disable" }]).output().map_err(|e| e.to_string())?;
    if o.status.success() {
        Ok(msg.into())
    } else {
        Err(String::from_utf8_lossy(&o.stderr).lines().last().unwrap_or("zpod-core-overlay failed").to_string())
    }
}

fn short(v: &str) -> String {
    v.split_whitespace().take(2).collect::<Vec<_>>().join(" ")
}

/// (line, is-warning) describing the running core and the last start
/// (each line fits the 320 px screen; a reason gets its own line).
pub fn summary(i: &Info) -> Vec<(String, bool)> {
    let mut v = Vec::new();
    if let Some(e) = &i.error {
        v.push((e.clone(), true));
        return v;
    }
    let running = if i.active == "update" { format!("update {}", short(&i.update)) } else { format!("image core {}", short(&i.image)) };
    v.push((format!("Running: {running}"), false));
    match i.state.as_str() {
        "ok" => v.push(("Last start: update loaded".into(), false)),
        "pending" => v.push(("Checking the update (launcher starting)".into(), false)),
        "fallback" => {
            v.push(("Fell back to the image core:".into(), true));
            v.push((i.reason.clone(), true));
        }
        "skipped" => {
            let warn = !i.reason.contains("Home");
            v.push(("Update not loaded this start:".into(), warn));
            v.push((i.reason.clone(), warn));
        }
        "off" => v.push(("Loading at boot is off".into(), false)),
        "manual" => v.push(("Update applied by hand (dev loop)".into(), false)),
        _ => v.push((if i.reason.is_empty() { "No update on the card".into() } else { i.reason.clone() }, false)),
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_status_kv() {
        let i = parse("boot_load=on\nboot_support=1\nactive=update\nimage=0.10.0 abc1234 2026-11-06\nupdate=0.10.0 def5678 2026-11-07\nstate=ok\nreason=\nfuture=1\n");
        assert!(i.boot_load && i.boot_support);
        assert_eq!(i.active, "update");
        assert_eq!(summary(&i)[0].0, "Running: update 0.10.0 def5678");
        assert!(!summary(&i)[1].1);
        let f = parse("boot_load=on\nactive=image\nimage=0.10.0\nstate=fallback\nreason=launcher not up within 60 s\n");
        assert_eq!(summary(&f)[1], ("Fell back to the image core:".into(), true));
        assert_eq!(summary(&f)[2], ("launcher not up within 60 s".into(), true));
        let h = parse("state=skipped\nreason=Home held at power-on\nactive=image\nimage=0.10.0\n");
        assert!(!summary(&h)[1].1, "a deliberate skip is not a warning");
        assert!(!parse("boot_load=off\n").boot_load);
    }
}
