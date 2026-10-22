//! ZPOD app system.
//!
//! An app lives in `/opt/zpod/apps/<id>/` with a `manifest.toml`:
//!
//! ```toml
//! [app]
//! id = "tetris"                 # = directory name: [a-z0-9][a-z0-9-]{0,31}
//! name = "Tetris"
//! version = "1.0.0"             # semver MAJOR.MINOR.PATCH
//! exec = "bin/tetris"           # relative path, executable
//! icon = "icon.png"             # optional, 32x32 PNG
//! description = "Falling blocks"
//! min_core = "0.5.0"            # oldest ZPOD core it runs on
//! order = 30                    # optional menu position (lower = higher)
//! ```
//!
//! A package (`<id>-<version>.zpa`) is a gzip'd tar of that directory with
//! `manifest.toml` at the top level.

pub mod elf;
pub mod manifest;
pub mod package;

pub use manifest::{AppInfo, Manifest, Version};

pub const DEFAULT_APPS_DIR: &str = "/opt/zpod/apps";

pub fn apps_dir() -> std::path::PathBuf {
    std::env::var_os("ZPOD_APPS_DIR").map(Into::into).unwrap_or_else(|| DEFAULT_APPS_DIR.into())
}

/// Version of the installed ZPOD core: /etc/zpod/core-version, else the
/// version these binaries were built as.
pub fn core_version() -> Version {
    let p = std::env::var("ZPOD_CORE_VERSION_FILE").unwrap_or_else(|_| "/etc/zpod/core-version".into());
    std::fs::read_to_string(p)
        .ok()
        .and_then(|s| Version::parse(s.trim()))
        .unwrap_or_else(|| Version::parse(zpod_core::CORE_VERSION).unwrap())
}

/// All valid installed apps, sorted for the menu (order, then name).
/// Invalid entries are returned separately with the reason.
pub fn discover(dir: &std::path::Path) -> (Vec<AppInfo>, Vec<(String, String)>) {
    let mut ok = Vec::new();
    let mut bad = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else { return (ok, bad) };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || !e.path().is_dir() {
            continue;
        }
        match AppInfo::load(&e.path()) {
            Ok(a) if a.manifest.app.id == name => ok.push(a),
            Ok(a) => bad.push((name, format!("manifest id '{}' != directory", a.manifest.app.id))),
            Err(err) => bad.push((name, err)),
        }
    }
    ok.sort_by(|a, b| (a.manifest.app.order, a.manifest.app.name.to_lowercase()).cmp(&(b.manifest.app.order, b.manifest.app.name.to_lowercase())));
    (ok, bad)
}
