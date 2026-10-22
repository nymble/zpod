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

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
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
}

fn default_order() -> i32 {
    100
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub app: AppSection,
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

impl Manifest {
    pub fn parse(text: &str) -> Result<Manifest, String> {
        let m: Manifest = toml::from_str(text).map_err(|e| format!("manifest.toml: {}", e.message()))?;
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
        Ok(())
    }

    pub fn version(&self) -> Version {
        Version::parse(&self.app.version).unwrap()
    }

    pub fn min_core(&self) -> Version {
        Version::parse(&self.app.min_core).unwrap()
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
        assert!(bad("order = 30", "order = 30\nroot = true").contains("unknown field"));
        assert!(bad("name = \"Tetris\"\n", "").contains("name"));
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
