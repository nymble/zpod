//! Display + input bundle used by every ZPOD app.

use std::io;
use std::time::{Duration, Instant};

use zpod_core::{Event, VolumeOverlay};
use zpod_ui::fb::Framebuffer;
use zpod_ui::widgets::{volume_overlay, VOLUME_OVERLAY};

use crate::{input::EvdevInput, lcd::Lcd, sim::Sim, sys::StatusSource};

pub struct Platform {
    pub fb: Framebuffer,
    pub lcd: Option<Lcd>,
    input: Option<EvdevInput>,
    pub sim: Option<Sim>,
    pub status: StatusSource,
    /// Side-button volume overlay, drawn here for every app (one look).
    pub volume: VolumeOverlay,
    epoch: Instant,
}

impl Platform {
    /// Real hardware, or the headless simulator when `ZPOD_SIM` is set.
    /// The panel gets the flash-free warm init unless nobody initialised it
    /// this boot (see [`crate::lcd::INIT_MARK`]).
    pub fn open() -> io::Result<Self> {
        Self::open_with(None)
    }

    pub fn open_with(cold: Option<bool>) -> io::Result<Self> {
        let fb = Framebuffer::default();
        if let Some(sim) = Sim::from_env() {
            return Ok(Platform { fb, lcd: None, input: None, sim: Some(sim), status: StatusSource::new(), volume: VolumeOverlay::default(), epoch: Instant::now() });
        }
        let lcd = Lcd::open(cold)?;
        Ok(Platform { fb, lcd: Some(lcd), input: Some(EvdevInput::new()), sim: None, status: StatusSource::new(), volume: VolumeOverlay::default(), epoch: Instant::now() })
    }

    pub fn is_sim(&self) -> bool {
        self.sim.is_some()
    }

    fn now_ms(&self) -> u64 {
        self.epoch.elapsed().as_millis() as u64
    }

    /// Push changed regions to the panel (no-op when nothing is dirty).
    ///
    /// While the volume overlay is visible it is composited on top: the
    /// app's pixels under it are saved, the overlay is drawn and sent, and
    /// the app's pixels are put back into the framebuffer (not sent), so the
    /// app never sees or has to redraw it. When it hides, that area is marked
    /// dirty and the next flush shows the app's real pixels again.
    pub fn flush(&mut self) {
        if self.volume.dirty {
            self.volume.dirty = false;
            let (x, y, w, h) = VOLUME_OVERLAY;
            self.fb.mark_dirty(x, y, x + w as i32 - 1, y + h as i32 - 1);
        }
        if !self.fb.is_dirty() {
            return;
        }
        let saved = if self.volume.visible() { Some(self.save_overlay_area()) } else { None };
        if saved.is_some() {
            volume_overlay(&mut self.fb, self.volume.pct, zpod_core::volume::VOLUME_CAP_PCT);
        }
        if let Some(l) = self.lcd.as_mut() {
            l.flush(&mut self.fb);
        } else if let Some(s) = self.sim.as_mut() {
            s.present(&mut self.fb);
        } else {
            self.fb.take_dirty();
        }
        if let Some(px) = saved {
            self.restore_overlay_area(&px);
        }
    }

    fn save_overlay_area(&self) -> Vec<u16> {
        let (x, y, w, h) = VOLUME_OVERLAY;
        let mut v = Vec::with_capacity((w * h) as usize);
        for yy in y as usize..(y as usize + h as usize) {
            for xx in x as usize..(x as usize + w as usize) {
                v.push(self.fb.get(xx, yy));
            }
        }
        v
    }

    fn restore_overlay_area(&mut self, px: &[u16]) {
        let (x, y, w, h) = VOLUME_OVERLAY;
        let mut i = 0;
        for yy in y as usize..(y as usize + h as usize) {
            for xx in x as usize..(x as usize + w as usize) {
                self.fb.px[yy * self.fb.width + xx] = px[i];
                i += 1;
            }
        }
    }

    fn raw_next_event(&mut self, timeout: Option<Duration>) -> Option<Event> {
        if let Some(s) = self.sim.as_mut() {
            return s.next_event(timeout);
        }
        match self.input.as_mut() {
            Some(i) => i.next_event(timeout),
            None => {
                std::thread::sleep(timeout.unwrap_or(Duration::from_secs(1)));
                None
            }
        }
    }

    /// Next key event (None on timeout). Volume key events update the shared
    /// overlay (shown while held, hidden 1.2 s after release) and are then
    /// passed on; apps must not draw their own volume display or change the
    /// volume (zpod-buttons already did).
    pub fn next_event(&mut self, timeout: Option<Duration>) -> Option<Event> {
        let deadline = timeout.map(|t| Instant::now() + t);
        loop {
            let now = self.now_ms();
            if self.volume.tick(now) {
                self.flush();
            }
            let mut wait = deadline.map(|d| d.saturating_duration_since(Instant::now()));
            if let Some(h) = self.volume.ms_until_hide(now) {
                let h = Duration::from_millis(h + 1);
                wait = Some(wait.map_or(h, |w| w.min(h)));
            }
            if let Some(e) = self.raw_next_event(wait) {
                let pct = if matches!(e.key, zpod_core::Key::VolUp | zpod_core::Key::VolDown) {
                    crate::sys::volume_get()
                } else {
                    None
                };
                let now = self.now_ms();
                if self.volume.on_event(&e, pct, now) {
                    self.status.set_volume(self.volume.pct);
                    self.flush();
                }
                return Some(e);
            }
            if deadline.is_some_and(|d| Instant::now() >= d) {
                let now = self.now_ms();
                if self.volume.tick(now) {
                    self.flush();
                }
                return None;
            }
        }
    }

    pub fn drain_input(&mut self) {
        if let Some(i) = self.input.as_mut() {
            i.drain();
        }
    }

    /// Release the panel (spidev + GPIO lines) so a child app can drive it.
    pub fn release_display(&mut self) {
        self.lcd = None;
    }

    /// Re-acquire the panel after a child exits and schedule a full redraw.
    pub fn reacquire_display(&mut self) -> io::Result<()> {
        if self.sim.is_none() && self.lcd.is_none() {
            let mut l = Lcd::open(None)?;
            l.invalidate();
            self.lcd = Some(l);
        }
        self.fb.mark_all_dirty();
        Ok(())
    }

    pub fn screenshot(&self, name: &str) {
        if let Some(s) = self.sim.as_ref() {
            let _ = s.save_png(name);
        }
    }
}

impl zpod_core::InputSource for Platform {
    fn next_event(&mut self, timeout_ms: Option<u32>) -> Option<Event> {
        Platform::next_event(self, timeout_ms.map(|m| Duration::from_millis(m as u64)))
    }
}
