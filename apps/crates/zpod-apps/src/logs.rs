//! Per-app logs and data directories.
//!
//! The launcher points each app's stdout and stderr at `<log dir>/<id>.log`
//! (`/data/zpod/logs` when `/data/zpod` exists, so logs survive a reboot and
//! can be read from the Mac with `zpod-app logs -f <id>`). Before each launch
//! a log over [`MAX_LOG`] bytes is moved to `<id>.log.1` (one old copy).

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Rotate a log at launch once it is bigger than this.
pub const MAX_LOG: u64 = 256 << 10;

/// `$ZPOD_LOG_DIR`, else `/data/zpod/logs` (Buildroot and Pi OS M5+ with a
/// data partition), else `/run/zpod/apps` (tmpfs).
pub fn log_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("ZPOD_LOG_DIR") {
        return d.into();
    }
    if Path::new("/data/zpod").is_dir() {
        "/data/zpod/logs".into()
    } else {
        "/run/zpod/apps".into()
    }
}

pub fn log_path(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.log"))
}

/// App data root, the same rule as the launcher (`set_data_dir`):
/// `$ZPOD_DATA_DIR`, else `/data/zpod/appdata`, else the app user's default.
pub fn data_root() -> PathBuf {
    if let Some(d) = std::env::var_os("ZPOD_DATA_DIR") {
        return d.into();
    }
    if Path::new("/data/zpod/appdata").is_dir() {
        "/data/zpod/appdata".into()
    } else {
        "/var/lib/zpod/home/.local/share/zpod".into()
    }
}

/// Open `<dir>/<id>.log` for appending (rotating it first if it is large).
pub fn open_for_launch(dir: &Path, id: &str) -> io::Result<File> {
    if !crate::manifest::valid_id(id) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid app id"));
    }
    fs::create_dir_all(dir)?;
    let p = log_path(dir, id);
    if fs::metadata(&p).map(|m| m.len() > MAX_LOG).unwrap_or(false) {
        let _ = fs::rename(&p, dir.join(format!("{id}.log.1")));
    }
    OpenOptions::new().create(true).append(true).open(&p)
}

/// Delete an app's data dir and logs. Returns what was removed.
pub fn purge(data_root: &Path, log_dir: &Path, id: &str) -> Result<Vec<PathBuf>, String> {
    if !crate::manifest::valid_id(id) {
        return Err(format!("invalid app id '{id}'"));
    }
    let mut gone = Vec::new();
    let d = data_root.join(id);
    if fs::symlink_metadata(&d).is_ok() {
        if d.is_dir() && !fs::symlink_metadata(&d).map(|m| m.file_type().is_symlink()).unwrap_or(false) {
            fs::remove_dir_all(&d).map_err(|e| format!("{}: {e}", d.display()))?;
        } else {
            fs::remove_file(&d).map_err(|e| format!("{}: {e}", d.display()))?;
        }
        gone.push(d);
    }
    for name in [format!("{id}.log"), format!("{id}.log.1")] {
        let p = log_dir.join(name);
        if fs::remove_file(&p).is_ok() {
            gone.push(p);
        }
    }
    Ok(gone)
}

/// The last `n` lines of a file (whole lines; at most the last 1 MiB is read).
pub fn tail(path: &Path, n: usize) -> io::Result<String> {
    let mut f = File::open(path)?;
    let len = f.metadata()?.len();
    let start = len.saturating_sub(1 << 20);
    f.seek(SeekFrom::Start(start))?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    let text = String::from_utf8_lossy(&buf);
    let lines: Vec<&str> = text.lines().collect();
    let from = lines.len().saturating_sub(n);
    let mut out = lines[from..].join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    Ok(out)
}

/// Print the last `n` lines, then follow the file like `tail -F`: new data is
/// written as it arrives, and a rotated or recreated log is reopened.
/// Returns on an output error, or when `alive()` turns false (checked every
/// 200 ms; the CLI checks that its reader, e.g. the ssh session, is still there).
pub fn follow(path: &Path, n: usize, out: &mut dyn Write, alive: &dyn Fn() -> bool) -> io::Result<()> {
    let mut pos = 0u64;
    let mut ino = 0u64;
    if let Ok(t) = tail(path, n) {
        out.write_all(t.as_bytes())?;
        out.flush()?;
        if let Ok(m) = fs::metadata(path) {
            pos = m.len();
            ino = m.ino();
        }
    }
    let mut buf = vec![0u8; 16 << 10];
    loop {
        match fs::metadata(path) {
            Ok(m) => {
                if m.ino() != ino || m.len() < pos {
                    ino = m.ino(); // rotated, recreated or truncated: start over
                    pos = 0;
                }
                if m.len() > pos {
                    let mut f = File::open(path)?;
                    f.seek(SeekFrom::Start(pos))?;
                    loop {
                        let k = f.read(&mut buf)?;
                        if k == 0 {
                            break;
                        }
                        out.write_all(&buf[..k])?;
                        pos += k as u64;
                    }
                    out.flush()?;
                }
            }
            Err(_) => {
                pos = 0;
                ino = 0;
            }
        }
        std::thread::sleep(Duration::from_millis(200));
        if !alive() {
            return Ok(());
        }
    }
}

/// False once nobody reads `fd` any more (pipe/socket peer closed). A tty
/// hangup sends SIGHUP instead; a regular file always counts as read.
pub fn reader_present(fd: i32) -> bool {
    let mut p = libc::pollfd { fd, events: 0, revents: 0 };
    let r = unsafe { libc::poll(&mut p, 1, 0) };
    !(r > 0 && p.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("zpod-logs-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn rotate_and_tail() {
        let d = tmp("rot");
        let mut f = open_for_launch(&d, "clock").unwrap();
        for i in 0..10 {
            writeln!(f, "line {i}").unwrap();
        }
        assert_eq!(tail(&log_path(&d, "clock"), 2).unwrap(), "line 8\nline 9\n");
        assert_eq!(tail(&log_path(&d, "clock"), 50).unwrap().lines().count(), 10);
        f.write_all(&vec![b'x'; MAX_LOG as usize + 1]).unwrap();
        drop(f);
        let mut f = open_for_launch(&d, "clock").unwrap();
        writeln!(f, "fresh").unwrap();
        assert_eq!(fs::read_to_string(log_path(&d, "clock")).unwrap(), "fresh\n");
        assert!(d.join("clock.log.1").exists());
        assert!(open_for_launch(&d, "../etc").is_err());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn purge_data_and_logs() {
        let root = tmp("purge");
        let (data, logs) = (root.join("appdata"), root.join("logs"));
        fs::create_dir_all(data.join("tetris/sub")).unwrap();
        fs::write(data.join("tetris/sub/scores"), "1").unwrap();
        fs::create_dir_all(data.join("clock")).unwrap();
        drop(open_for_launch(&logs, "tetris").unwrap());
        let gone = purge(&data, &logs, "tetris").unwrap();
        assert_eq!(gone.len(), 2);
        assert!(!data.join("tetris").exists() && !log_path(&logs, "tetris").exists());
        assert!(data.join("clock").exists(), "other apps' data kept");
        assert!(purge(&data, &logs, "tetris").unwrap().is_empty());
        assert!(purge(&data, &logs, "..").is_err());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn follow_sees_appends() {
        let d = tmp("follow");
        let p = log_path(&d, "a");
        fs::write(&p, "old1\nold2\n").unwrap();
        struct Sink(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
        impl Write for Sink {
            fn write(&mut self, b: &[u8]) -> io::Result<usize> {
                let mut v = self.0.lock().unwrap();
                if v.len() > 1000 {
                    return Err(io::Error::other("enough"));
                }
                v.extend_from_slice(b);
                Ok(b.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let got = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let g2 = got.clone();
        let p2 = p.clone();
        std::thread::spawn(move || {
            let _ = follow(&p2, 1, &mut Sink(g2), &|| true);
        });
        std::thread::sleep(Duration::from_millis(300));
        OpenOptions::new().append(true).open(&p).unwrap().write_all(b"new\n").unwrap();
        std::thread::sleep(Duration::from_millis(600));
        assert_eq!(String::from_utf8(got.lock().unwrap().clone()).unwrap(), "old2\nnew\n");
        // a closed pipe reader is noticed
        let mut fds = [0i32; 2];
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        assert!(reader_present(fds[1]));
        unsafe { libc::close(fds[0]) };
        assert!(!reader_present(fds[1]));
        unsafe { libc::close(fds[1]) };
        let _ = fs::remove_dir_all(&d);
    }
}
