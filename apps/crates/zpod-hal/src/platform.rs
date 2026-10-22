//! Display + input bundle used by every ZPOD app.

use std::io;
use std::time::Duration;

use zpod_core::Event;
use zpod_ui::fb::Framebuffer;

use crate::{input::EvdevInput, lcd::Lcd, sim::Sim, sys::StatusSource};

pub struct Platform {
    pub fb: Framebuffer,
    pub lcd: Option<Lcd>,
    input: Option<EvdevInput>,
    pub sim: Option<Sim>,
    pub status: StatusSource,
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
            return Ok(Platform { fb, lcd: None, input: None, sim: Some(sim), status: StatusSource::new() });
        }
        let lcd = Lcd::open(cold)?;
        Ok(Platform { fb, lcd: Some(lcd), input: Some(EvdevInput::new()), sim: None, status: StatusSource::new() })
    }

    pub fn is_sim(&self) -> bool {
        self.sim.is_some()
    }

    /// Push changed regions to the panel (no-op when nothing is dirty).
    pub fn flush(&mut self) {
        if !self.fb.is_dirty() {
            return;
        }
        if let Some(l) = self.lcd.as_mut() {
            l.flush(&mut self.fb);
        } else if let Some(s) = self.sim.as_mut() {
            s.present(&mut self.fb);
        } else {
            self.fb.take_dirty();
        }
    }

    pub fn next_event(&mut self, timeout: Option<Duration>) -> Option<Event> {
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
