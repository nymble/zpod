use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub u32, pub u32, pub u32);

impl Version {
    /// "1.2.3" (a "-suffix"/"+build" is accepted and ignored for ordering).
    pub fn parse(s: &str) -> Option<Version> {
        let core = s.trim().trim_start_matches('v').split(['-', '+']).next()?;
        let mut it = core.split('.');
        let v = Version(it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next().unwrap_or("0").parse().ok()?);
        if it.next().is_some() {
            return None;
        }
        Some(v)
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

/// Manifest schema this code understands (docs/APP-MANIFEST.md). A manifest
/// without `schema` is schema 0 (test3 and older). Fields added within a
/// schema are additive: older readers ignore them. A breaking change bumps
/// the schema and older readers refuse the manifest.
pub const SCHEMA: u32 = 1;

/// Launcher API level these crates implement (`requires.launcher_api`).
/// 1 = test3 launcher, 2 = control socket + USB role (ctl 2), 3 = menu tree,
/// `[menu]`/`[[handles]]`/`[requires]`, boot-time core updates.
pub const LAUNCHER_API: u32 = 3;

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct AppSection {
    pub id: String,
    pub name: String,
    pub version: String,
    pub exec: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub description: String,
    pub min_core: String,
    #[serde(default = "default_order")]
    pub order: i32,
    /// Informational: target the binaries were built for.
    #[serde(default)]
    pub arch: Option<String>,
    /// Legacy placement (schema 0): "app" = Home, "tool" = Tools. Schema 1
    /// uses `[menu] path`; when both are given `[menu]` wins.
    #[serde(default)]
    pub category: Option<String>,
    /// Extra things the launcher lets this app do. Known: "usb-host" (ask
    /// the launcher to switch the USB port between computer link and USB
    /// devices; see ctl.rs). Needs a launcher from 2026-11-05 (ctl 2).
    #[serde(default)]
    pub permissions: Vec<String>,
}

/// `[menu]`: where the app lives in the launcher's menu tree.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct MenuSection {
    /// Folder path, e.g. "Tools/Radio"; "" = Home. Must start at a built-in
    /// folder ([`crate::tree::FOLDERS`]); deeper folders are made on demand.
    #[serde(default)]
    pub path: Option<String>,
    /// Position inside the folder (lower = higher); default `app.order`.
    #[serde(default)]
    pub order: Option<i32>,
    /// Menu label; default `app.name`.
    #[serde(default)]
    pub label: Option<String>,
    /// Extra placements (the same app listed in more than one folder).
    #[serde(default)]
    pub also: Vec<AlsoSection>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct AlsoSection {
    pub path: String,
    #[serde(default)]
    pub order: Option<i32>,
    #[serde(default)]
    pub label: Option<String>,
}

/// `[[handles]]`: other ways to reach the app than its menu entry.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Handle {
    /// usb-device | serial | file | input. Unknown kinds are ignored (warning).
    pub kind: String,
    /// What it matches: "067b:2303" (usb-device VID:PID, or "class:02"),
    /// "nmea" (serial protocol), "mid" (file extension), "keyboard" (input).
    #[serde(rename = "match")]
    pub matches: Vec<String>,
    /// Shown in the chooser, e.g. "GPS receiver".
    #[serde(default)]
    pub label: Option<String>,
}

/// `[requires]`: what the app needs. Install checks `launcher_api`; the
/// launcher hides apps it cannot run and lists why under Installed apps.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Requires {
    /// Minimum launcher API ([`LAUNCHER_API`]).
    #[serde(default)]
    pub launcher_api: Option<u32>,
    /// Needs the USB port in host mode (the app asks before switching).
    #[serde(default)]
    pub usb_host: bool,
    /// Needs a Wi-Fi network (client or hotspot).
    #[serde(default)]
    pub wifi: bool,
    /// Inputs the app works with: buttons, keyboard, mouse, touch.
    #[serde(default)]
    pub input: Vec<String>,
}

/// Values accepted in `permissions`.
pub const KNOWN_PERMISSIONS: &[&str] = &["usb-host"];
/// `[[handles]] kind` values this core acts on.
pub const KNOWN_HANDLES: &[&str] = &["usb-device", "serial", "file", "input"];
/// `requires.input` values.
pub const KNOWN_INPUTS: &[&str] = &["buttons", "keyboard", "mouse", "touch"];

impl AppSection {
    /// Legacy: listed under Tools (schema 0 `category = "tool"`).
    pub fn is_tool(&self) -> bool {
        self.category.as_deref() == Some("tool")
    }

    pub fn has_permission(&self, p: &str) -> bool {
        self.permissions.iter().any(|x| x == p)
    }
}

fn default_order() -> i32 {
    100
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Manifest {
    /// 0 (absent) or 1.
    #[serde(default)]
    pub schema: u32,
    pub app: AppSection,
    #[serde(default)]
    pub menu: Option<MenuSection>,
    #[serde(default)]
    pub handles: Vec<Handle>,
    #[serde(default)]
    pub requires: Requires,
    /// Keys this reader does not know (ignored), as "table.key".
    #[serde(skip)]
    pub unknown: Vec<String>,
}

pub fn valid_id(id: &str) -> bool {
    let b = id.as_bytes();
    !b.is_empty()
        && b.len() <= 32
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

/// Relative path inside the app dir, no "..", no absolute, no empty parts.
pub fn safe_rel(p: &str) -> bool {
    !p.is_empty()
        && !p.starts_with('/')
        && !p.contains('\\')
        && p.split('/').all(|c| !c.is_empty() && c != ".." && c != ".")
}

/// Keys each table knows; anything else is reported in `Manifest::unknown`.
const KNOWN_KEYS: &[(&str, &[&str])] = &[
    ("", &["schema", "app", "menu", "handles", "requires"]),
    ("app", &["id", "name", "version", "exec", "args", "icon", "description", "min_core", "order", "arch", "category", "permissions"]),
    ("menu", &["path", "order", "label", "also"]),
    ("menu.also", &["path", "order", "label"]),
    ("handles", &["kind", "match", "label"]),
    ("requires", &["launcher_api", "usb_host", "wifi", "input"]),
];

fn unknown_keys(t: &toml::Table, table: &str, out: &mut Vec<String>) {
    let known = KNOWN_KEYS.iter().find(|(n, _)| *n == table).map(|(_, k)| *k).unwrap_or(&[]);
    for (k, v) in t {
        let name = if table.is_empty() { k.clone() } else { format!("{table}.{k}") };
        if !known.contains(&k.as_str()) {
            out.push(name);
            continue;
        }
        match v {
            toml::Value::Table(sub) => unknown_keys(sub, &name, out),
            toml::Value::Array(a) => {
                for x in a {
                    if let toml::Value::Table(sub) = x {
                        unknown_keys(sub, &name, out);
                    }
                }
            }
            _ => {}
        }
    }
}

/// `match` accepts a string or a list of strings.
fn normalise_handles(t: &mut toml::Table) {
    if let Some(toml::Value::Array(hs)) = t.get_mut("handles") {
        for h in hs.iter_mut() {
            if let toml::Value::Table(h) = h {
                if let Some(toml::Value::String(s)) = h.get("match").cloned() {
                    h.insert("match".into(), toml::Value::Array(vec![toml::Value::String(s)]));
                }
            }
        }
    }
}

impl Manifest {
    /// Parse and validate. Unknown keys are not errors (they are listed in
    /// `unknown`); a schema newer than [`SCHEMA`] is.
    pub fn parse(text: &str) -> Result<Manifest, String> {
        let mut t: toml::Table = text.parse().map_err(|e: toml::de::Error| format!("manifest.toml: {}", e.message()))?;
        if let Some(v) = t.get("schema") {
            let s = v.as_integer().ok_or("manifest.toml: schema must be a number")?;
            if s < 0 || s > SCHEMA as i64 {
                return Err(format!("manifest schema {s} needs a newer ZPOD (this one reads schema <= {SCHEMA})"));
            }
        }
        let mut unknown = Vec::new();
        unknown_keys(&t, "", &mut unknown);
        normalise_handles(&mut t);
        let mut m: Manifest = toml::Value::Table(t).try_into().map_err(|e: toml::de::Error| format!("manifest.toml: {}", e.message()))?;
        m.unknown = unknown;
        m.validate()?;
        Ok(m)
    }

    pub fn validate(&self) -> Result<(), String> {
        let a = &self.app;
        if !valid_id(&a.id) {
            return Err(format!("invalid id '{}' (use a-z, 0-9, '-', max 32)", a.id));
        }
        if a.name.trim().is_empty() || a.name.len() > 40 {
            return Err("name must be 1..40 characters".into());
        }
        Version::parse(&a.version).ok_or_else(|| format!("invalid version '{}'", a.version))?;
        Version::parse(&a.min_core).ok_or_else(|| format!("invalid min_core '{}'", a.min_core))?;
        if !safe_rel(&a.exec) {
            return Err(format!("exec must be a relative path inside the app: '{}'", a.exec));
        }
        if let Some(i) = &a.icon {
            if !safe_rel(i) {
                return Err(format!("bad icon path '{i}'"));
            }
        }
        for p in &a.permissions {
            if !KNOWN_PERMISSIONS.contains(&p.as_str()) {
                return Err(format!("unknown permission '{p}' (known: {})", KNOWN_PERMISSIONS.join(", ")));
            }
        }
        if let Some(c) = &a.category {
            if c != "app" && c != "tool" {
                return Err(format!("category must be \"app\" or \"tool\", not '{c}'"));
            }
        }
        if let Some(m) = &self.menu {
            if let Some(l) = &m.label {
                if l.trim().is_empty() || l.len() > 40 {
                    return Err("menu.label must be 1..40 characters".into());
                }
            }
        }
        // placement (menu path, also, handles) is checked against the
        // built-in folders by crate::tree::check, which also warns
        Ok(())
    }

    pub fn version(&self) -> Version {
        Version::parse(&self.app.version).unwrap()
    }

    pub fn min_core(&self) -> Version {
        Version::parse(&self.app.min_core).unwrap()
    }

    /// Launcher API the app needs (1 when it does not say).
    pub fn launcher_api(&self) -> u32 {
        self.requires.launcher_api.unwrap_or(1)
    }
}

/// An installed app.
#[derive(Clone, Debug)]
pub struct AppInfo {
    pub dir: PathBuf,
    pub manifest: Manifest,
}

impl AppInfo {
    pub fn load(dir: &Path) -> Result<AppInfo, String> {
        let text = std::fs::read_to_string(dir.join("manifest.toml")).map_err(|e| format!("manifest.toml: {e}"))?;
        let manifest = Manifest::parse(&text)?;
        Ok(AppInfo { dir: dir.to_path_buf(), manifest })
    }
    pub fn exec_path(&self) -> PathBuf {
        self.dir.join(&self.manifest.app.exec)
    }
    pub fn icon_path(&self) -> Option<PathBuf> {
        self.manifest.app.icon.as_ref().map(|i| self.dir.join(i))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"
[app]
id = "tetris"
name = "Tetris"
version = "1.0.0"
exec = "bin/tetris"
icon = "icon.png"
description = "Falling blocks"
min_core = "0.5.0"
order = 30
"#;

    #[test]
    fn parses_good_manifest() {
        let m = Manifest::parse(GOOD).unwrap();
        assert_eq!(m.app.id, "tetris");
        assert_eq!(m.version(), Version(1, 0, 0));
        assert_eq!(m.app.order, 30);
        assert!(m.app.args.is_empty());
    }

    #[test]
    fn rejects_bad_manifests() {
        let bad = |from: &str, to: &str| Manifest::parse(&GOOD.replace(from, to)).unwrap_err();
        assert!(bad("id = \"tetris\"", "id = \"Tetris!\"").contains("invalid id"));
        assert!(bad("bin/tetris", "../../bin/sh").contains("exec"));
        assert!(bad("bin/tetris", "/bin/sh").contains("exec"));
        assert!(bad("version = \"1.0.0\"", "version = \"one\"").contains("version"));
        assert!(bad("min_core = \"0.5.0\"", "min_core = \"x\"").contains("min_core"));
        assert!(bad("name = \"Tetris\"\n", "").contains("name"));
        assert!(bad("order = 30", "order = 30\npermissions = [\"root\"]").contains("unknown permission"));
    }

    #[test]
    fn permissions() {
        let m = Manifest::parse(GOOD).unwrap();
        assert!(m.app.permissions.is_empty() && !m.app.has_permission("usb-host"));
        let m = Manifest::parse(&GOOD.replace("order = 30", "order = 30\npermissions = [\"usb-host\"]")).unwrap();
        assert!(m.app.has_permission("usb-host"));
    }

    #[test]
    fn versions_order() {
        assert!(Version::parse("1.10.0").unwrap() > Version::parse("1.9.9").unwrap());
        assert_eq!(Version::parse("v0.5.0-m4"), Some(Version(0, 5, 0)));
        assert_eq!(Version::parse("2.1"), Some(Version(2, 1, 0)));
        assert_eq!(Version::parse("1.2.3.4"), None);
        assert!(valid_id("my-app2") && !valid_id("-x") && !valid_id("") && !valid_id("a_b"));
        assert!(safe_rel("bin/x") && !safe_rel("bin/../x") && !safe_rel("./x"));
    }
}
