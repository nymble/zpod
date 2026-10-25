//! ZPOD Linux platform layer (Pi Zero W, Raspberry Pi OS / Buildroot).
//!
//! * [`lcd::Lcd`]: the panel through `mipidsi` + `zpod-panel` over
//!   linux-embedded-hal (`/dev/spidev0.0`, DC = GPIO25 via gpio-cdev, GPIO27
//!   held high), flushing only the changed parts of the framebuffer.
//! * [`input`]: buttons from the `zpod-buttons` uinput keyboard via evdev
//!   (the buttons daemon stays the single GPIO owner).
//! * [`sys`]: battery (MAX17048 via embedded-hal driver on /dev/i2c-1),
//!   interface addresses, ALSA 'Digital' volume, Wi-Fi, local time.
//! * [`platform::Platform`]: display + input bundle; with `ZPOD_SIM=<dir>` it
//!   runs headless (scripted keys, PNG screenshots) on any Linux host.
//!
//! Nothing here depends on systemd.

pub mod conf;
pub mod input;
pub mod lcd;
pub mod platform;
pub mod sim;
pub mod storage;
pub mod sys;

pub use platform::Platform;
pub use zpod_core::{Event, Key, KeyState, StatusInfo};
pub use zpod_ui::fb::{Framebuffer, HEIGHT, WIDTH};

/// Directory for per-app persistent data (settings, high scores).
pub fn data_dir(app: &str) -> std::path::PathBuf {
    let base = std::env::var_os("ZPOD_DATA_DIR")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/share/zpod")))
        .unwrap_or_else(|| "/var/lib/zpod".into());
    let d = base.join(app);
    let _ = std::fs::create_dir_all(&d);
    d
}

/// File-backed [`zpod_core::Storage`] in [`data_dir`].
pub struct FileStorage(pub std::path::PathBuf);

impl FileStorage {
    pub fn for_app(app: &str) -> Self {
        FileStorage(data_dir(app))
    }
}

impl zpod_core::Storage for FileStorage {
    fn load(&mut self, key: &str, buf: &mut [u8]) -> Option<usize> {
        let v = std::fs::read(self.0.join(key)).ok()?;
        let n = v.len().min(buf.len());
        buf[..n].copy_from_slice(&v[..n]);
        Some(n)
    }
    fn save(&mut self, key: &str, value: &[u8]) -> bool {
        let p = self.0.join(key);
        let tmp = p.with_extension("tmp");
        std::fs::write(&tmp, value).and_then(|_| std::fs::rename(&tmp, &p)).is_ok()
    }
}
