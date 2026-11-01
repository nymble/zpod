//! ZPOD platform-agnostic core (`no_std`, no allocator required).
//!
//! Everything an app core needs from "the device" goes through the small
//! traits here, so the same app logic runs on the Pi Zero (Linux), in the
//! desktop simulator, and later on an ESP32-S3 (esp-idf std or no_std).
#![no_std]

pub mod input;
pub mod time;
pub mod volume;
pub mod nav;

pub use input::{Event, Key, KeyState};
pub use time::DateTime;
pub use nav::{classify, Nav};
pub use volume::VolumeOverlay;

/// Source of button events. `timeout_ms = None` blocks; `Some(0)` polls.
pub trait InputSource {
    fn next_event(&mut self, timeout_ms: Option<u32>) -> Option<Event>;
}

/// Wall clock (local time) and a monotonic millisecond counter.
pub trait Clock {
    fn now_local(&self) -> DateTime;
    fn monotonic_ms(&self) -> u64;
}

/// Small key/value persistence (settings, high scores). Keys are short ASCII
/// names, values are opaque bytes (apps use tiny text formats).
pub trait Storage {
    /// Copy the stored value into `buf`; returns the length, or None if absent.
    fn load(&mut self, key: &str, buf: &mut [u8]) -> Option<usize>;
    fn save(&mut self, key: &str, value: &[u8]) -> bool;
}

/// Battery fuel gauge.
pub trait Battery {
    /// (state of charge %, cell volts), None if unreadable.
    fn read(&mut self) -> Option<(f32, f32)>;
}

/// PCM output for a future native decoder (ESP32 I2S, ALSA). The Pi build
/// plays music through MPD instead, so nothing implements this on Linux yet.
pub trait AudioSink {
    type Error;
    fn sample_rate(&self) -> u32;
    /// Interleaved stereo i16 frames. Returns frames consumed.
    fn write(&mut self, frames: &[i16]) -> Result<usize, Self::Error>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PlayState {
    #[default]
    Unknown,
    Stopped,
    Playing,
    Paused,
}

/// Wi-Fi mode as the status bar shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum WifiMode {
    /// not determined yet (draws like the old on/off icon from `wifi`)
    #[default]
    Unknown,
    /// radio off / interface down
    Off,
    /// client: `bars` 0..=4 from the link signal, `connected` = has an IPv4
    Client { bars: u8, connected: bool },
    /// ZPOD is the access point; `clients` = associated stations
    Hotspot { clients: u8 },
}

/// Signal bars 0..=4 from dBm (same thresholds as the Tools lists).
pub fn wifi_bars(dbm: i32) -> u8 {
    match dbm {
        d if d >= -55 => 4,
        d if d >= -65 => 3,
        d if d >= -75 => 2,
        d if d >= -85 => 1,
        _ => 0,
    }
}

/// Everything the status bar shows. Filled in by the platform layer.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct StatusInfo {
    pub time: Option<DateTime>,
    pub battery_pct: Option<f32>,
    pub charging: bool,
    pub wifi: bool,
    pub wifi_mode: WifiMode,
    pub usb: bool,
    pub volume_pct: Option<u8>,
    pub play: PlayState,
}

/// Version of the ZPOD core (image) this crate belongs to. Apps declare
/// `min_core` in their manifest and the installer compares against this.
pub const CORE_VERSION: &str = env!("CARGO_PKG_VERSION");
