//! SD card usage per partition and the image's build identity
//! (/etc/zpod-release, written at image build time). Used by the launcher's
//! Battery & about, System status and Diagnostics > Storage & build screens.

use crate::sys::sim;
use std::os::unix::fs::MetadataExt;

pub const RELEASE_FILE: &str = "/etc/zpod-release";

/// Build identity from /etc/zpod-release (KEY=VALUE lines, os-release style).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Release {
    /// Image file stem, e.g. "2026-10-25-zpod-pi0w-m4.1-candidate".
    pub image: String,
    /// Human version, e.g. "m4.1-candidate" or "v0.5.0-m4".
    pub version: String,
    /// Short git commit, "+dirty" when built from a modified tree.
    pub git: String,
    /// Build date (YYYY-MM-DD).
    pub date: String,
}

impl Release {
    pub fn parse(s: &str) -> Release {
        let mut r = Release::default();
        for line in s.lines() {
            let l = line.trim();
            if l.starts_with('#') {
                continue;
            }
            let Some((k, v)) = l.split_once('=') else { continue };
            let v = v.trim().trim_matches(|c| c == '"' || c == '\'').to_string();
            match k.trim() {
                "ZPOD_IMAGE" => r.image = v,
                "ZPOD_VERSION" => r.version = v,
                "ZPOD_GIT" => r.git = v,
                "ZPOD_BUILD_DATE" => r.date = v,
                _ => {}
            }
        }
        r
    }

    pub fn is_known(&self) -> bool {
        !self.version.is_empty() || !self.git.is_empty()
    }

    /// Version + commit for one narrow row: "m4.1-candidate  ab12cd3".
    pub fn brief(&self) -> String {
        if !self.is_known() {
            return "unknown".into();
        }
        [self.version.as_str(), self.git.as_str()].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join("  ")
    }

    /// Image name without the leading build date ("zpod-pi0w-m4.1-candidate").
    pub fn image_name(&self) -> &str {
        let b = self.image.as_bytes();
        if b.len() > 11 && b[4] == b'-' && b[7] == b'-' && b[10] == b'-' && b[..4].iter().all(u8::is_ascii_digit) {
            &self.image[11..]
        } else {
            &self.image
        }
    }

    /// One line: "m4.1-candidate  ab12cd3  2026-10-25".
    pub fn short(&self) -> String {
        if !self.is_known() {
            return "unknown (no /etc/zpod-release)".into();
        }
        [self.version.as_str(), self.git.as_str(), self.date.as_str()]
            .iter()
            .filter(|s| !s.is_empty())
            .copied()
            .collect::<Vec<_>>()
            .join("  ")
    }
}

/// `ZPOD_RELEASE_FILE` overrides the path (tests, simulator).
pub fn release() -> Release {
    let p = std::env::var("ZPOD_RELEASE_FILE").ok();
    if p.is_none() && sim() {
        return Release {
            image: "2026-10-25-zpod-pi0w-m4.1-candidate".into(),
            version: "m4.1-candidate".into(),
            git: "0123abc".into(),
            date: "2026-10-25".into(),
        };
    }
    std::fs::read_to_string(p.as_deref().unwrap_or(RELEASE_FILE)).map(|s| Release::parse(&s)).unwrap_or_default()
}

/// Bytes on one mounted file system (df semantics: used = total - free,
/// percent = used / (used + available)).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FsUsage {
    pub total: u64,
    pub used: u64,
    pub avail: u64,
    pub dev: u64,
}

impl FsUsage {
    pub fn pct(&self) -> u32 {
        let d = self.used + self.avail;
        if d == 0 { 0 } else { ((self.used * 100 + d - 1) / d) as u32 }
    }
    /// "14%  1.9 GB / 14.3 GB"
    pub fn line(&self) -> String {
        format!("{}%  {} / {}", self.pct(), human(self.used), human(self.total))
    }
}

pub fn fs_usage(path: &str) -> Option<FsUsage> {
    if sim() {
        const G: u64 = 1 << 30;
        const M: u64 = 1 << 20;
        return match path {
            "/" | "/srv/zpod/media" | "/srv/zpod/media/music" => Some(FsUsage { total: 14 * G + 300 * M, used: 2 * G + 400 * M, avail: 11 * G + 300 * M, dev: 1 }),
            "/boot/firmware" => Some(FsUsage { total: 510 * M, used: 64 * M, avail: 446 * M, dev: 2 }),
            _ => None,
        };
    }
    let c = std::ffi::CString::new(path).ok()?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        return None;
    }
    let f = st.f_frsize as u64;
    let total = st.f_blocks as u64 * f;
    let free = st.f_bfree as u64 * f;
    let avail = st.f_bavail as u64 * f;
    let dev = std::fs::metadata(path).map(|m| m.dev()).unwrap_or(0);
    Some(FsUsage { total, used: total.saturating_sub(free), avail, dev })
}

/// Raw size of the SD card (mmcblk0), whatever the partitions use.
pub fn sd_card_bytes() -> Option<u64> {
    if sim() {
        return Some(15_931_539_456); // a "16 GB" card
    }
    let s = std::fs::read_to_string("/sys/block/mmcblk0/size").ok()?;
    s.trim().parse::<u64>().ok().map(|n| n * 512)
}

/// Binary units like `df -h`, labelled GB/MB for the small screen.
pub fn human(b: u64) -> String {
    const K: f64 = 1024.0;
    let f = b as f64;
    if f >= K * K * K {
        let g = f / (K * K * K);
        if g >= 100.0 { format!("{g:.0} GB") } else { format!("{g:.1} GB") }
    } else if f >= K * K {
        format!("{:.0} MB", f / (K * K))
    } else {
        format!("{:.0} kB", f / K)
    }
}

/// The ext4 data partition (M5 large-card layout): music, playlists and app
/// data live here; root stays small and fixed.
pub const DATA: &str = "/data";

/// The partitions shown on screen: (label, mount point).
pub const PARTS: [(&str, &str); 4] = [("Root /", "/"), ("Data", DATA), ("Music", "/srv/zpod/media"), ("Boot", "/boot/firmware")];

/// One line per partition (label, text, nearly full >= 90 %). The music
/// folder says "on data" (M5 layout, bind mount from /data/media) or
/// "on root" (older single-partition images). /data is skipped when it is
/// not a separate file system.
pub fn partition_lines() -> Vec<(&'static str, String, bool)> {
    let root = fs_usage("/");
    let data = fs_usage(DATA).filter(|d| root.map(|r| r.dev != d.dev).unwrap_or(true));
    PARTS
        .iter()
        .filter_map(|&(label, path)| {
            if path == DATA && data.is_none() {
                return None;
            }
            let u = fs_usage(path);
            Some(match u {
                Some(u) if path != "/" && path != DATA && data.map(|d| d.dev == u.dev).unwrap_or(false) => (label, format!("on data  ({} free)", human(u.avail)), false),
                Some(u) if path != "/" && root.map(|r| r.dev == u.dev).unwrap_or(false) => (label, format!("on root  ({} free)", human(u.avail)), false),
                Some(u) => (label, u.line(), u.pct() >= 90),
                None => (label, "not mounted".into(), false),
            })
        })
        .collect()
}

/// Short SD summary for one row: "16 GB card, root 17% used".
pub fn sd_summary() -> String {
    let card = sd_card_bytes().map(|b| format!("{} card", human_card(b)));
    let root = fs_usage("/").map(|u| format!("root {}% used", u.pct()));
    match (card, root) {
        (Some(c), Some(r)) => format!("{c}, {r}"),
        (Some(c), None) => c,
        (None, Some(r)) => r,
        (None, None) => "n/a".into(),
    }
}

/// Card capacity the way it is sold (decimal, nearest power of two GB).
pub fn human_card(b: u64) -> String {
    let gb = b as f64 / 1e9;
    let mut n = 1u64;
    while (n as f64) * 1.1 < gb && n < 4096 {
        n *= 2;
    }
    if gb < 1.0 { human(b) } else { format!("{n} GB") }
}

/// Login account (uid 1000) and whether first-boot setup finished. On an
/// unconfigured card (no userconf.txt / zpod-ssh.pub) SSH logins fail.
pub fn login_state() -> (String, bool) {
    if sim() {
        return ("paul".into(), true);
    }
    let name = std::fs::read_to_string("/etc/passwd")
        .ok()
        .and_then(|t| t.lines().find(|l| l.split(':').nth(2) == Some("1000")).map(|l| l.split(':').next().unwrap_or("").to_string()))
        .unwrap_or_default();
    let ready = !std::path::Path::new("/etc/ssh/sshd_config.d/rename_user.conf").exists();
    (name, ready)
}

/// "paul" or "NOT SET UP (userconf.txt)".
pub fn login_line() -> String {
    match login_state() {
        (n, true) if !n.is_empty() => n,
        (_, true) => "none (no uid 1000)".into(),
        (_, false) => "NOT SET UP: userconf.txt / zpod-ssh.pub".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_release() {
        let r = Release::parse("# ZPOD build identity\nZPOD_IMAGE=2026-10-25-zpod-pi0w-m4.1-candidate\nZPOD_VERSION=\"m4.1-candidate\"\nZPOD_GIT='ab12cd3+dirty'\nZPOD_BUILD_DATE=2026-10-25\nOTHER=x\n");
        assert_eq!(r.image, "2026-10-25-zpod-pi0w-m4.1-candidate");
        assert_eq!(r.version, "m4.1-candidate");
        assert_eq!(r.git, "ab12cd3+dirty");
        assert_eq!(r.short(), "m4.1-candidate  ab12cd3+dirty  2026-10-25");
        assert_eq!(r.brief(), "m4.1-candidate  ab12cd3+dirty");
        assert_eq!(r.image_name(), "zpod-pi0w-m4.1-candidate");
        assert_eq!(Release { image: "custom".into(), ..Default::default() }.image_name(), "custom");
        assert!(!Release::parse("").is_known());
        assert!(Release::parse("").short().starts_with("unknown"));
    }

    #[test]
    fn usage_maths() {
        let u = FsUsage { total: 1000, used: 250, avail: 700, dev: 0 };
        assert_eq!(u.pct(), 27); // df rounds up: 250 / 950
        assert_eq!(FsUsage { total: 2 << 30, used: 1 << 30, avail: 1 << 30, dev: 0 }.line(), "50%  1.0 GB / 2.0 GB");
        assert_eq!(FsUsage { total: 0, used: 0, avail: 0, dev: 0 }.pct(), 0);
        assert_eq!(human(3 * (1 << 30) / 2), "1.5 GB");
        assert_eq!(human(200 << 30), "200 GB");
        assert_eq!(human(64 << 20), "64 MB");
        assert_eq!(human_card(15_931_539_456), "16 GB");
        assert_eq!(human_card(31_914_983_424), "32 GB");
        assert_eq!(human_card(7_948_206_080), "8 GB");
    }

    #[test]
    fn real_statvfs_root() {
        if sim() {
            return;
        }
        let u = fs_usage("/").expect("statvfs /");
        assert!(u.total > 0 && u.used <= u.total);
    }
}
