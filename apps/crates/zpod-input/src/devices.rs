//! Input device discovery through sysfs (`/sys/class/input/eventN`), no
//! ioctls: name, ids and capability bitmaps decide keyboard vs mouse.

use std::path::{Path, PathBuf};

use crate::event::SourceKind;

/// The onboard buttons' uinput device: apps read it through zpod-hal
/// already, so the HID reader skips it (its KEY_I/J/K/U are not letters).
pub const BUTTONS_DEVICE: &str = "zpod-buttons";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    /// "event3"
    pub node: String,
    pub dev: PathBuf,
    pub name: String,
    pub kind: SourceKind,
    pub bus: u16,
    pub vendor: u16,
    pub product: u16,
}

/// Parse a sysfs capability bitmap ("120013" or "1000 0 0 7fffff ffffffff"
/// words, most significant first, each `long`-sized) and test `bit`.
pub fn bitmap_has(text: &str, bit: usize, word_bits: usize) -> bool {
    let words: Vec<&str> = text.split_whitespace().collect();
    let idx = bit / word_bits;
    if idx >= words.len() {
        return false;
    }
    let w = words[words.len() - 1 - idx];
    u64::from_str_radix(w, 16).map(|v| v >> (bit % word_bits) & 1 == 1).unwrap_or(false)
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).map(|s| s.trim().to_string()).unwrap_or_default()
}

fn hex(p: &Path) -> u16 {
    u16::from_str_radix(&read(p), 16).unwrap_or(0)
}

/// Keyboard: has KEY_A..KEY_Z (and Enter). Mouse: REL_X + REL_Y + BTN_LEFT.
pub fn classify(ev: &str, key: &str, rel: &str, word_bits: usize) -> Option<SourceKind> {
    let has_ev = |b| bitmap_has(ev, b, word_bits);
    let key_has = |b| bitmap_has(key, b, word_bits);
    let rel_has = |b| bitmap_has(rel, b, word_bits);
    if has_ev(2) && rel_has(0) && rel_has(1) && key_has(0x110) {
        return Some(SourceKind::Mouse);
    }
    // Q, A, Z, Enter, Space
    if has_ev(1) && [16, 30, 44, 28, 57].iter().all(|&k| key_has(k)) {
        return Some(SourceKind::Keyboard);
    }
    None
}

/// All keyboards and mice under `root` (normally /sys/class/input), sorted.
pub fn scan(root: &Path, dev_dir: &Path) -> Vec<Candidate> {
    let word_bits = std::mem::size_of::<libc::c_ulong>() * 8;
    let Ok(rd) = std::fs::read_dir(root) else { return vec![] };
    let mut v: Vec<Candidate> = rd
        .flatten()
        .filter_map(|e| {
            let node = e.file_name().to_string_lossy().into_owned();
            if !node.starts_with("event") {
                return None;
            }
            let d = e.path().join("device");
            let name = read(&d.join("name"));
            if name == BUTTONS_DEVICE {
                return None;
            }
            let caps = d.join("capabilities");
            let kind = classify(&read(&caps.join("ev")), &read(&caps.join("key")), &read(&caps.join("rel")), word_bits)?;
            Some(Candidate {
                dev: dev_dir.join(&node),
                node,
                name: if name.is_empty() { "input device".into() } else { name },
                kind,
                bus: hex(&d.join("id/bustype")),
                vendor: hex(&d.join("id/vendor")),
                product: hex(&d.join("id/product")),
            })
        })
        .collect();
    v.sort_by_key(|c| c.node[5..].parse::<u32>().unwrap_or(u32::MAX));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitmap_words() {
        assert!(bitmap_has("120013", 0, 64));
        assert!(bitmap_has("120013", 1, 64));
        assert!(!bitmap_has("120013", 2, 64));
        assert!(bitmap_has("120013", 20, 64));
        // 32-bit words: bit 33 = word 1 (second from the right) bit 1
        assert!(bitmap_has("2 0", 33, 32));
        assert!(!bitmap_has("2 0", 1, 32));
        assert!(!bitmap_has("ff", 100, 32));
        assert!(!bitmap_has("zz", 0, 32));
    }

    fn fake(root: &Path, node: &str, name: &str, ev: &str, key: &str, rel: &str) {
        let d = root.join(node).join("device");
        std::fs::create_dir_all(d.join("capabilities")).unwrap();
        std::fs::create_dir_all(d.join("id")).unwrap();
        std::fs::write(d.join("name"), format!("{name}\n")).unwrap();
        std::fs::write(d.join("capabilities/ev"), ev).unwrap();
        std::fs::write(d.join("capabilities/key"), key).unwrap();
        std::fs::write(d.join("capabilities/rel"), rel).unwrap();
        std::fs::write(d.join("id/bustype"), "0003").unwrap();
        std::fs::write(d.join("id/vendor"), "046d").unwrap();
        std::fs::write(d.join("id/product"), "c077").unwrap();
    }

    #[test]
    fn scan_fixture_tree() {
        let wb = std::mem::size_of::<libc::c_ulong>() * 8;
        let root = std::env::temp_dir().join(format!("zpod-input-sysfs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        // keyboard: KEY_* bits 1..=127 set
        let kb_key = if wb == 64 { "ffffffffffffffff fffffffffffffffe" } else { "ffffffff ffffffff ffffffff fffffffe" };
        // mouse: BTN_LEFT..BTN_TASK = bits 0x110..0x117 -> word with bits 16..23 of word 8 (32) / word 4 (64)
        let mouse_key = if wb == 64 { "ff0000 0 0 0 0" } else { "ff0000 0 0 0 0 0 0 0 0" };
        fake(&root, "event0", BUTTONS_DEVICE, "3", kb_key, "0");
        fake(&root, "event1", "PixArt USB Optical Mouse", "17", mouse_key, "903");
        fake(&root, "event10", "Apple Keyboard", "120013", kb_key, "0");
        fake(&root, "event2", "gpio-keys", "3", "1", "0");
        let v = scan(&root, Path::new("/dev/input"));
        let got: Vec<_> = v.iter().map(|c| (c.node.as_str(), c.kind, c.name.as_str())).collect();
        assert_eq!(got, vec![("event1", SourceKind::Mouse, "PixArt USB Optical Mouse"), ("event10", SourceKind::Keyboard, "Apple Keyboard")]);
        assert_eq!((v[0].bus, v[0].vendor, v[0].product), (3, 0x046d, 0xc077));
        assert_eq!(v[1].dev, PathBuf::from("/dev/input/event10"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
