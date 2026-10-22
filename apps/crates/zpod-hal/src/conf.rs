//! Tiny INI access for /etc/zpod/ui.conf (`ZPOD_UI_CONF` overrides the path).
//! Only what the launcher needs: read a value, set a value in place.

pub fn path() -> std::path::PathBuf {
    std::env::var_os("ZPOD_UI_CONF").map(Into::into).unwrap_or_else(|| "/etc/zpod/ui.conf".into())
}

/// Value of `key` in `[section]` (comments `#`/`;` and surrounding blanks stripped).
pub fn get_in(text: &str, section: &str, key: &str) -> Option<String> {
    let mut cur = String::new();
    for line in text.lines() {
        let l = line.trim();
        if l.starts_with('[') && l.ends_with(']') {
            cur = l[1..l.len() - 1].trim().to_string();
            continue;
        }
        if cur != section || l.starts_with('#') || l.starts_with(';') {
            continue;
        }
        if let Some((k, v)) = l.split_once('=') {
            if k.trim() == key {
                let v = v.split(['#', ';']).next().unwrap_or("").trim();
                return Some(v.to_string());
            }
        }
    }
    None
}

/// Return `text` with `key = value` set in `[section]` (added if missing).
pub fn set_in(text: &str, section: &str, key: &str, value: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut done = false;
    let mut sec_seen = false;
    for line in text.lines() {
        let l = line.trim();
        if l.starts_with('[') && l.ends_with(']') {
            if cur == section && !done {
                out.push(format!("{key} = {value}"));
                done = true;
            }
            cur = l[1..l.len() - 1].trim().to_string();
            sec_seen |= cur == section;
            out.push(line.to_string());
            continue;
        }
        if cur == section && !done && !l.starts_with('#') && !l.starts_with(';') {
            if let Some((k, _)) = l.split_once('=') {
                if k.trim() == key {
                    out.push(format!("{key} = {value}"));
                    done = true;
                    continue;
                }
            }
        }
        out.push(line.to_string());
    }
    if !done {
        if !sec_seen {
            out.push(String::new());
            out.push(format!("[{section}]"));
        }
        out.push(format!("{key} = {value}"));
    }
    let mut s = out.join("\n");
    s.push('\n');
    s
}

pub fn get(section: &str, key: &str) -> Option<String> {
    get_in(&std::fs::read_to_string(path()).unwrap_or_default(), section, key)
}

pub fn set(section: &str, key: &str, value: &str) -> std::io::Result<()> {
    let p = path();
    let t = std::fs::read_to_string(&p).unwrap_or_default();
    let tmp = p.with_extension("tmp");
    std::fs::write(&tmp, set_in(&t, section, key, value))?;
    std::fs::rename(tmp, p)
}

#[cfg(test)]
mod tests {
    use super::*;
    const T: &str = "[shutdown]\n# c\nscreen = message\n\n[display]\nspi_mhz = 32  # safe\n";
    #[test]
    fn get_values() {
        assert_eq!(get_in(T, "shutdown", "screen").as_deref(), Some("message"));
        assert_eq!(get_in(T, "display", "spi_mhz").as_deref(), Some("32"));
        assert_eq!(get_in(T, "display", "nope"), None);
    }
    #[test]
    fn set_existing_new_key_new_section() {
        let a = set_in(T, "display", "spi_mhz", "48");
        assert_eq!(get_in(&a, "display", "spi_mhz").as_deref(), Some("48"));
        assert_eq!(get_in(&a, "shutdown", "screen").as_deref(), Some("message"));
        let b = set_in(T, "shutdown", "extra", "1");
        assert_eq!(get_in(&b, "shutdown", "extra").as_deref(), Some("1"));
        assert_eq!(get_in(&b, "display", "spi_mhz").as_deref(), Some("32"));
        let c = set_in(T, "battery", "shutdown_pct", "5");
        assert_eq!(get_in(&c, "battery", "shutdown_pct").as_deref(), Some("5"));
    }
}
