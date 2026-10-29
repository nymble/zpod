//! Alarm and timer ringing (M5). The Clock app stores the settings
//! (zpod_hal::alarm, `<data dir>/clock/alarm.conf`); the launcher, which is
//! always running, rings them: it stops a foreground app, pauses MPD, takes
//! the DAC from MPD (`disableoutput`; the hw device is exclusive), loops a
//! beep WAV through `aplay -D default`, and gives everything back afterwards.
//! Alarm: OK/Circle/Right/Home/Play = stop; any other button = snooze.
//! Timer: any button stops. Volume buttons stay global. Auto-stop after 5 min.
//! There is no wake from power-off: the alarm only rings while the ZPOD runs.

use std::process::{Child, Command, Stdio};
use std::time::Instant;

use zpod_core::Key;
use zpod_hal::alarm::{self, Ring};
use zpod_hal::sys;

pub const WAV: &str = "/run/zpod/alarm.wav";
/// Minimum mixer level while ringing (restored afterwards).
pub const MIN_VOLUME: u8 = 60;

pub struct Ringer {
    pub ring: Ring,
    pub started: Instant,
    child: Option<Child>,
    output_off: bool,
    restore_volume: Option<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RingKey {
    Ignore,
    Stop,
    Snooze,
}

/// Which keys stop and which snooze.
pub fn classify(ring: Ring, k: Key) -> RingKey {
    match (ring, k) {
        (_, Key::VolUp | Key::VolDown) => RingKey::Ignore,
        (Ring::Timer, _) => RingKey::Stop,
        (_, Key::Ok | Key::Circle | Key::Right | Key::Home | Key::PlayPause) => RingKey::Stop,
        _ => RingKey::Snooze,
    }
}

fn amixer_set(pct: u8) {
    let _ = Command::new("amixer")
        .args(["-q", "-c", sys::CARD, "sset", sys::MIXER, &format!("{pct}%")])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

impl Ringer {
    pub fn start(ring: Ring) -> Ringer {
        let mut r = Ringer { ring, started: Instant::now(), child: None, output_off: false, restore_volume: None };
        if sys::sim() {
            return r;
        }
        if let Ok(mut c) = zpod_mpd::Client::connect_default() {
            let _ = c.simple("pause 1");
            r.output_off = c.simple("disableoutput 0").is_ok();
        }
        if let Some(v) = sys::volume_get() {
            if v < MIN_VOLUME {
                r.restore_volume = Some(v);
                amixer_set(MIN_VOLUME);
            }
        }
        if !std::path::Path::new(WAV).exists() {
            let _ = std::fs::create_dir_all("/run/zpod");
            let _ = std::fs::write(WAV, alarm::beep_wav(2));
        }
        let mut cmd = Command::new("sh");
        cmd.args(["-c", &format!("while :; do aplay -q -D default {WAV} || sleep 1; done")])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
        r.child = cmd.spawn().map_err(|e| eprintln!("launcher: alarm sound: {e}")).ok();
        r
    }

    pub fn expired(&self) -> bool {
        self.started.elapsed().as_secs() >= alarm::RING_MAX_S
    }

    pub fn stop(&mut self) {
        if let Some(mut c) = self.child.take() {
            crate::proc::stop_child(&mut c, std::time::Duration::from_millis(300));
        }
        if let Some(v) = self.restore_volume.take() {
            amixer_set(v);
        }
        if self.output_off {
            if let Ok(mut c) = zpod_mpd::Client::connect_default() {
                let _ = c.simple("enableoutput 0");
            }
            self.output_off = false;
        }
    }
}

impl Drop for Ringer {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_stop_or_snooze() {
        let a = Ring::Alarm(0);
        assert_eq!(classify(a, Key::Ok), RingKey::Stop);
        assert_eq!(classify(a, Key::Home), RingKey::Stop);
        assert_eq!(classify(a, Key::PlayPause), RingKey::Stop);
        assert_eq!(classify(a, Key::Up), RingKey::Snooze);
        assert_eq!(classify(a, Key::Cross), RingKey::Snooze);
        assert_eq!(classify(a, Key::VolUp), RingKey::Ignore);
        assert_eq!(classify(Ring::Timer, Key::Up), RingKey::Stop);
        assert_eq!(classify(Ring::Timer, Key::VolDown), RingKey::Ignore);
    }
}
