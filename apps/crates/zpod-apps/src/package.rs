//! `.zpa` packages: gzip'd tar of an app directory, `manifest.toml` at the root.
//!
//! Installation is staged and atomic: the archive is extracted (with strict
//! path/type checks) into `<apps>/.staging-<id>-<pid>`, validated (manifest,
//! exec present, ELF arch is ARMv6-compatible, min_core satisfied), then
//! swapped into place with renames. A failed install leaves the old version
//! untouched.

use crate::elf;
use crate::manifest::{Manifest, Version};
use flate2::read::GzDecoder;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};

pub const MAX_TOTAL_BYTES: u64 = 256 << 20;
pub const MAX_ENTRIES: usize = 10_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Fresh install; refuse if already installed.
    Install,
    /// Must already be installed; refuse downgrades / same version.
    Update,
    /// Install or update, whatever is there (allows downgrade / reinstall).
    Force,
}

#[derive(Debug, PartialEq)]
pub struct Installed {
    pub id: String,
    pub name: String,
    pub from: Option<Version>,
    pub to: Version,
    pub warnings: Vec<String>,
}

/// Turn a tar member path into a safe relative path, or None to skip it ("./").
fn clean(path: &Path) -> Result<Option<PathBuf>, String> {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::Normal(s) => out.push(s),
            _ => return Err(format!("unsafe path in package: {}", path.display())),
        }
    }
    Ok(if out.as_os_str().is_empty() { None } else { Some(out) })
}

/// Read only the manifest out of a package.
pub fn inspect(pkg: &Path) -> Result<Manifest, String> {
    let f = File::open(pkg).map_err(|e| format!("{}: {e}", pkg.display()))?;
    let mut ar = tar::Archive::new(GzDecoder::new(f));
    for e in ar.entries().map_err(|e| format!("not a .zpa (gzip tar): {e}"))? {
        let e = e.map_err(|e| format!("corrupt package: {e}"))?;
        let p = e.path().map_err(|e| e.to_string())?.into_owned();
        if clean(&p)?.as_deref() == Some(Path::new("manifest.toml")) {
            let mut s = String::new();
            e.take(64 * 1024).read_to_string(&mut s).map_err(|e| e.to_string())?;
            return Manifest::parse(&s);
        }
    }
    Err("package has no top-level manifest.toml".into())
}

fn extract(pkg: &Path, dest: &Path) -> Result<(), String> {
    let f = File::open(pkg).map_err(|e| format!("{}: {e}", pkg.display()))?;
    let mut ar = tar::Archive::new(GzDecoder::new(f));
    let mut total = 0u64;
    let mut count = 0usize;
    fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    for e in ar.entries().map_err(|e| format!("not a .zpa (gzip tar): {e}"))? {
        let mut e = e.map_err(|e| format!("corrupt package: {e}"))?;
        count += 1;
        if count > MAX_ENTRIES {
            return Err("too many files in package".into());
        }
        let raw = e.path().map_err(|e| e.to_string())?.into_owned();
        let Some(rel) = clean(&raw)? else { continue };
        let target = dest.join(&rel);
        let ty = e.header().entry_type();
        if ty.is_dir() {
            fs::create_dir_all(&target).map_err(|e| e.to_string())?;
            fs::set_permissions(&target, fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
        } else if ty.is_file() {
            let mode = e.header().mode().unwrap_or(0o644);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            if target.exists() {
                return Err(format!("duplicate entry {}", rel.display()));
            }
            let mut out = File::create(&target).map_err(|e| format!("{}: {e}", target.display()))?;
            let budget = MAX_TOTAL_BYTES - total;
            let n = io::copy(&mut (&mut e).take(budget + 1), &mut out).map_err(|e| e.to_string())?;
            total += n;
            if total > MAX_TOTAL_BYTES {
                return Err("package too large (>256 MiB unpacked)".into());
            }
            out.flush().map_err(|e| e.to_string())?;
            let m = if mode & 0o111 != 0 { 0o755 } else { 0o644 };
            fs::set_permissions(&target, fs::Permissions::from_mode(m)).map_err(|e| e.to_string())?;
        } else if matches!(ty, tar::EntryType::XGlobalHeader | tar::EntryType::XHeader | tar::EntryType::GNULongName) {
            continue;
        } else {
            return Err(format!("unsupported entry type for {} (symlinks/devices are not allowed)", rel.display()));
        }
    }
    Ok(())
}

fn walk_files(dir: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    let mut v: Vec<_> = fs::read_dir(dir)?.flatten().collect();
    v.sort_by_key(|e| e.file_name());
    for e in v {
        let ft = e.file_type()?;
        if ft.is_dir() {
            walk_files(&e.path(), out)?;
        } else if ft.is_file() {
            out.push(e.path());
        }
    }
    Ok(())
}

/// Validate an unpacked app tree. Returns the manifest and warnings.
pub fn validate_tree(dir: &Path, core: Version) -> Result<(Manifest, Vec<String>), String> {
    let text = fs::read_to_string(dir.join("manifest.toml")).map_err(|_| "package has no top-level manifest.toml".to_string())?;
    let m = Manifest::parse(&text)?;
    let mut warnings = Vec::new();
    if m.min_core() > core {
        return Err(format!("{} needs ZPOD core >= {}, this device has {}", m.app.id, m.app.min_core, core));
    }
    let exec = dir.join(&m.app.exec);
    let meta = fs::symlink_metadata(&exec).map_err(|_| format!("exec '{}' missing from package", m.app.exec))?;
    if !meta.is_file() {
        return Err(format!("exec '{}' is not a regular file", m.app.exec));
    }
    fs::set_permissions(&exec, fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    // Check every ELF in the package, not just exec (helpers, plugins).
    let mut files = Vec::new();
    walk_files(dir, &mut files).map_err(|e| e.to_string())?;
    for f in &files {
        let mut head = Vec::new();
        File::open(f).and_then(|h| h.take(4).read_to_end(&mut head)).map_err(|e| e.to_string())?;
        if head != b"\x7fELF" {
            continue;
        }
        let bytes = fs::read(f).map_err(|e| e.to_string())?;
        let rel = f.strip_prefix(dir).unwrap_or(f).display().to_string();
        match elf::check(&bytes) {
            elf::Check::Incompatible(why) => return Err(format!("{rel}: {why}")),
            elf::Check::Ok { arch: None } => warnings.push(format!("{rel}: no ARM build attributes; cannot confirm ARMv6")),
            _ => {}
        }
    }
    if let Some(icon) = &m.app.icon {
        if !dir.join(icon).is_file() {
            warnings.push(format!("icon '{icon}' missing; the launcher will use a default"));
        }
    }
    Ok((m, warnings))
}

struct Lock(File);
impl Lock {
    fn take(apps: &Path) -> Result<Lock, String> {
        fs::create_dir_all(apps).map_err(|e| format!("{}: {e}", apps.display()))?;
        let f = File::create(apps.join(".lock")).map_err(|e| format!("{}: {e} (run with sudo?)", apps.display()))?;
        use std::os::fd::AsRawFd;
        if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err("another zpod-app operation is running".into());
        }
        Ok(Lock(f))
    }
}
impl Drop for Lock {
    fn drop(&mut self) {
        use std::os::fd::AsRawFd;
        unsafe { libc::flock(self.0.as_raw_fd(), libc::LOCK_UN) };
    }
}

pub fn install(pkg: &Path, apps: &Path, mode: Mode, core: Version) -> Result<Installed, String> {
    let _lock = Lock::take(apps)?;
    let pre = inspect(pkg)?;
    let id = pre.app.id.clone();
    let dest = apps.join(&id);
    let current = crate::AppInfo::load(&dest).ok().map(|a| a.manifest.version());
    let new_v = pre.version();
    match (mode, current) {
        (Mode::Install, Some(v)) => return Err(format!("{id} {v} is already installed; use 'zpod-app update' (or --force)")),
        (Mode::Update, None) => return Err(format!("{id} is not installed; use 'zpod-app install'")),
        (Mode::Update, Some(v)) if new_v <= v => {
            return Err(format!("{id}: installed {v} is not older than {new_v} (use --force to reinstall/downgrade)"))
        }
        _ => {}
    }
    let pid = std::process::id();
    let staging = apps.join(format!(".staging-{id}-{pid}"));
    let _ = fs::remove_dir_all(&staging);
    let result = (|| {
        extract(pkg, &staging)?;
        let (m, warnings) = validate_tree(&staging, core)?;
        if m.app.id != id {
            return Err("manifest changed during extraction".to_string());
        }
        let old = apps.join(format!(".old-{id}-{pid}"));
        let had_old = dest.exists();
        if had_old {
            fs::rename(&dest, &old).map_err(|e| format!("moving old version aside: {e}"))?;
        }
        if let Err(e) = fs::rename(&staging, &dest) {
            if had_old {
                let _ = fs::rename(&old, &dest);
            }
            return Err(format!("installing: {e}"));
        }
        if had_old {
            let _ = fs::remove_dir_all(&old);
        }
        // Make the rename durable before reporting success (power can be cut any time).
        if let Ok(d) = File::open(apps) {
            let _ = d.sync_all();
        }
        Ok(Installed { id: id.clone(), name: m.app.name.clone(), from: current, to: new_v, warnings })
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

pub fn remove(apps: &Path, id: &str) -> Result<Version, String> {
    if !crate::manifest::valid_id(id) {
        return Err(format!("invalid app id '{id}'"));
    }
    let _lock = Lock::take(apps)?;
    let dest = apps.join(id);
    let info = crate::AppInfo::load(&dest).map_err(|_| format!("{id} is not installed"))?;
    let trash = apps.join(format!(".removing-{id}-{}", std::process::id()));
    fs::rename(&dest, &trash).map_err(|e| e.to_string())?;
    fs::remove_dir_all(&trash).map_err(|e| e.to_string())?;
    Ok(info.manifest.version())
}

/// Build `<out_dir>/<id>-<version>.zpa` from an app directory. Entries are
/// sorted, owned by root, mtime = SOURCE_DATE_EPOCH (or 0): reproducible.
pub fn pack(src: &Path, out_dir: &Path) -> Result<PathBuf, String> {
    let text = fs::read_to_string(src.join("manifest.toml")).map_err(|e| format!("{}/manifest.toml: {e}", src.display()))?;
    let m = Manifest::parse(&text)?;
    let exec = src.join(&m.app.exec);
    if !exec.is_file() {
        return Err(format!("exec '{}' not found in {}", m.app.exec, src.display()));
    }
    let mtime: u64 = std::env::var("SOURCE_DATE_EPOCH").ok().and_then(|s| s.parse().ok()).unwrap_or(0);
    let out = out_dir.join(format!("{}-{}.zpa", m.app.id, m.app.version));
    let f = File::create(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let gz = flate2::write::GzEncoder::new(f, flate2::Compression::best());
    let mut tb = tar::Builder::new(gz);
    let mut files = Vec::new();
    walk_files(src, &mut files).map_err(|e| e.to_string())?;
    // manifest first so inspect() finds it immediately
    files.sort_by_key(|p| (p.file_name() != Some("manifest.toml".as_ref()) || p.parent() != Some(src), p.clone()));
    for f in files {
        let rel = f.strip_prefix(src).unwrap();
        let meta = fs::metadata(&f).map_err(|e| e.to_string())?;
        let mut h = tar::Header::new_gnu();
        h.set_size(meta.len());
        let exe = meta.permissions().mode() & 0o111 != 0 || f == exec;
        h.set_mode(if exe { 0o755 } else { 0o644 });
        h.set_uid(0);
        h.set_gid(0);
        h.set_mtime(mtime);
        h.set_entry_type(tar::EntryType::Regular);
        tb.append_data(&mut h, rel, File::open(&f).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    }
    tb.into_inner().and_then(|gz| gz.finish()).map_err(|e| e.to_string())?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("zpa-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn app_src(root: &Path, version: &str, min_core: &str) -> PathBuf {
        let s = root.join(format!("src-{version}"));
        fs::create_dir_all(s.join("bin")).unwrap();
        fs::write(
            s.join("manifest.toml"),
            format!("[app]\nid = \"hello\"\nname = \"Hello\"\nversion = \"{version}\"\nexec = \"bin/hello\"\nicon = \"icon.png\"\ndescription = \"test\"\nmin_core = \"{min_core}\"\n"),
        )
        .unwrap();
        fs::write(s.join("bin/hello"), "#!/bin/sh\necho hello\n").unwrap();
        fs::write(s.join("icon.png"), b"not really a png").unwrap();
        s
    }

    const CORE: Version = Version(0, 5, 0);

    #[test]
    fn pack_install_update_remove() {
        let root = tmp("cycle");
        let apps = root.join("apps");
        let p1 = pack(&app_src(&root, "1.0.0", "0.5.0"), &root).unwrap();
        assert_eq!(p1.file_name().unwrap(), "hello-1.0.0.zpa");
        assert_eq!(inspect(&p1).unwrap().app.id, "hello");

        let r = install(&p1, &apps, Mode::Install, CORE).unwrap();
        assert_eq!((r.from, r.to), (None, Version(1, 0, 0)));
        let exe = apps.join("hello/bin/hello");
        assert_eq!(fs::metadata(&exe).unwrap().permissions().mode() & 0o777, 0o755);
        assert!(install(&p1, &apps, Mode::Install, CORE).unwrap_err().contains("already installed"));
        assert!(install(&p1, &apps, Mode::Update, CORE).unwrap_err().contains("not older"));

        let p2 = pack(&app_src(&root, "1.1.0", "0.5.0"), &root).unwrap();
        let r = install(&p2, &apps, Mode::Update, CORE).unwrap();
        assert_eq!(r.from, Some(Version(1, 0, 0)));
        let (list, bad) = crate::discover(&apps);
        assert_eq!(list.len(), 1);
        assert!(bad.is_empty());
        assert_eq!(list[0].manifest.app.version, "1.1.0");
        // downgrade only with force
        assert!(install(&p1, &apps, Mode::Update, CORE).is_err());
        assert_eq!(install(&p1, &apps, Mode::Force, CORE).unwrap().to, Version(1, 0, 0));
        // no staging leftovers
        let names: Vec<_> = fs::read_dir(&apps).unwrap().flatten().map(|e| e.file_name().into_string().unwrap()).collect();
        assert!(names.iter().all(|n| n == "hello" || n == ".lock"), "{names:?}");

        assert_eq!(remove(&apps, "hello").unwrap(), Version(1, 0, 0));
        assert!(remove(&apps, "hello").is_err());
        assert!(remove(&apps, "../etc").is_err());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn rejects_newer_core_requirement() {
        let root = tmp("core");
        let p = pack(&app_src(&root, "1.0.0", "9.0.0"), &root).unwrap();
        let e = install(&p, &root.join("apps"), Mode::Install, CORE).unwrap_err();
        assert!(e.contains("needs ZPOD core >= 9.0.0"), "{e}");
        assert!(!root.join("apps/hello").exists());
        fs::remove_dir_all(&root).unwrap();
    }

    fn raw_tar(root: &Path, entries: &[(&str, &[u8], tar::EntryType)]) -> PathBuf {
        let out = root.join("evil.zpa");
        let gz = flate2::write::GzEncoder::new(File::create(&out).unwrap(), flate2::Compression::fast());
        let mut tb = tar::Builder::new(gz);
        for (name, data, ty) in entries {
            let mut h = tar::Header::new_gnu();
            // write the raw name bytes so "../" survives (append_data would reject it)
            h.as_gnu_mut().unwrap().name[..name.len()].copy_from_slice(name.as_bytes());
            h.set_size(data.len() as u64);
            h.set_mode(0o755);
            h.set_entry_type(*ty);
            if *ty == tar::EntryType::Symlink {
                h.set_link_name("/etc/passwd").unwrap();
            }
            h.set_cksum();
            tb.append(&h, *data).unwrap();
        }
        tb.into_inner().unwrap().finish().unwrap();
        out
    }

    #[test]
    fn rejects_traversal_and_symlinks() {
        let root = tmp("evil");
        let apps = root.join("apps");
        let manifest = b"[app]\nid = \"evil\"\nname = \"Evil\"\nversion = \"1.0.0\"\nexec = \"run\"\nmin_core = \"0.1.0\"\n";
        let reg = tar::EntryType::Regular;
        let p = raw_tar(&root, &[("manifest.toml", manifest, reg), ("run", b"#!/bin/sh\n", reg), ("../../escape", b"x", reg)]);
        assert!(install(&p, &apps, Mode::Install, CORE).unwrap_err().contains("unsafe path"));
        assert!(!root.join("escape").exists() && !apps.join("evil").exists());

        let p = raw_tar(&root, &[("manifest.toml", manifest, reg), ("run", b"", tar::EntryType::Symlink)]);
        assert!(install(&p, &apps, Mode::Install, CORE).unwrap_err().contains("not allowed"));

        let p = raw_tar(&root, &[("manifest.toml", manifest, reg), ("run", b"\x7fELF\x02\x01\x01\0\0\0\0\0\0\0\0\0\x02\0\x3e\0\x01\0\0\0aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", reg)]);
        assert!(install(&p, &apps, Mode::Install, CORE).unwrap_err().contains("64-bit"));

        let p = raw_tar(&root, &[("run", b"#!/bin/sh\n", reg)]);
        assert!(install(&p, &apps, Mode::Install, CORE).unwrap_err().contains("manifest"));
        let leftovers: Vec<_> = fs::read_dir(&apps).unwrap().flatten().map(|e| e.file_name()).filter(|n| n != ".lock").collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
        fs::remove_dir_all(&root).unwrap();
    }
}
