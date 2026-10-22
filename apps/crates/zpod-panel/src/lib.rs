//! ZPOD panel model for [`mipidsi`].
//!
//! The panel is an ILI9340-style controller (240x320 native, used landscape).
//! mipidsi's stock `ILI9341Rgb565` model sends a different init (it writes
//! 0xB4 and orders MADCTL/COLMOD/SLPOUT differently), so this crate provides
//! a custom [`Model`] that replays the exact, hardware-confirmed sequence of
//! `overlay/usr/lib/zpod/zpod_lcd.py` (zdisp.py candidate 1 + zorient.py):
//!
//! ```text
//! SWRESET, 150 ms, SLPOUT, 150 ms, COLMOD 0x55, MADCTL 0x08, INVOFF,
//! NORON, DISPON, 50 ms, MADCTL 0xE8 (MY|MX|MV|BGR) -> 320x240 landscape
//! ```
//!
//! The board has no reset pin. mipidsi sends SWRESET itself when no reset pin
//! is given ([`cold_builder`]). For a hand-off between processes (boot splash
//! -> launcher -> app) the panel is already initialised, so [`warm_builder`]
//! passes a no-op reset pin (mipidsi then skips SWRESET) and the model sends
//! only SLPOUT/COLMOD/MADCTL/NORON/DISPON with no long sleeps: no flash.
//!
//! The same code drives the panel on Linux (linux-embedded-hal spidev +
//! gpio-cdev) and on an ESP32 (esp-idf-hal / esp-hal SPI).
#![no_std]

use embedded_graphics::pixelcolor::Rgb565;
use embedded_hal::{delay::DelayNs, digital::OutputPin};
use mipidsi::{
    dcs::{InterfaceExt, SetAddressMode},
    interface::Interface,
    models::{Model, ModelInitError},
    options::{ColorOrder, ModelOptions, Orientation, Rotation},
    Builder, NoResetPin,
};

pub const WIDTH: u16 = 320;
pub const HEIGHT: u16 = 240;
/// Hardware-confirmed landscape MADCTL value.
pub const MADCTL_LANDSCAPE: u8 = 0xE8;

/// mipidsi orientation that yields MADCTL 0xE8 together with BGR:
/// Deg270 -> (MY=1, MX=0) ^ mirrored -> MX=1, MV=1.
pub const ORIENTATION: Orientation = Orientation { rotation: Rotation::Deg270, mirrored: true };
pub const COLOR_ORDER: ColorOrder = ColorOrder::Bgr;

/// The ZPOD panel. `warm = true`: panel already initialised by another
/// process; skip reset and long sleeps.
pub struct ZpodIli9340 {
    pub warm: bool,
}

impl Model for ZpodIli9340 {
    type ColorFormat = Rgb565;
    const FRAMEBUFFER_SIZE: (u16, u16) = (240, 320);

    fn init<DELAY, DI>(
        &mut self,
        di: &mut DI,
        delay: &mut DELAY,
        options: &ModelOptions,
    ) -> Result<SetAddressMode, ModelInitError<DI::Error>>
    where
        DELAY: DelayNs,
        DI: Interface,
    {
        let madctl = SetAddressMode::from(options);
        if self.warm {
            di.write_raw(0x11, &[])?; // SLPOUT (no-op when awake)
            delay.delay_ms(5);
            di.write_raw(0x3A, &[0x55])?; // COLMOD 16 bit
            di.write_command(madctl)?;
            di.write_raw(0x13, &[])?; // NORON
            di.write_raw(0x29, &[])?; // DISPON
            return Ok(madctl);
        }
        // SWRESET was just sent by mipidsi (no reset pin).
        delay.delay_ms(150);
        di.write_raw(0x11, &[])?; // SLPOUT
        delay.delay_ms(150);
        di.write_raw(0x3A, &[0x55])?; // COLMOD 16-bit
        di.write_raw(0x36, &[0x08])?; // MADCTL BGR (confirmed init)
        di.write_raw(0x20, &[])?; // INVOFF
        di.write_raw(0x13, &[])?; // NORON
        di.write_raw(0x29, &[])?; // DISPON
        delay.delay_ms(50);
        di.write_command(madctl)?; // 0xE8 landscape
        Ok(madctl)
    }
}

/// Reset "pin" that does nothing: makes mipidsi skip its SWRESET.
pub struct NoopReset;
impl embedded_hal::digital::ErrorType for NoopReset {
    type Error = core::convert::Infallible;
}
impl OutputPin for NoopReset {
    fn set_low(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
    fn set_high(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

fn base<DI: Interface<Word = u8>>(di: DI, warm: bool) -> Builder<DI, ZpodIli9340, NoResetPin> {
    Builder::new(ZpodIli9340 { warm }, di)
        .display_size(240, 320) // native portrait size; MV swaps to 320x240
        .orientation(ORIENTATION)
        .color_order(COLOR_ORDER)
}

/// Full init (power-on / boot splash): SWRESET + confirmed sequence (~350 ms).
pub fn cold_builder<DI: Interface<Word = u8>>(di: DI) -> Builder<DI, ZpodIli9340, NoResetPin> {
    base(di, false)
}

/// Hand-off init (launcher/app after splash): no reset, no flash (~5 ms).
pub fn warm_builder<DI: Interface<Word = u8>>(di: DI) -> Builder<DI, ZpodIli9340, NoopReset> {
    base(di, true).reset_pin(NoopReset)
}

/// Initialise the panel with a single display type for both cases:
/// `cold = true` sends SWRESET + the full confirmed sequence (~350 ms),
/// `cold = false` is the flash-free hand-off init.
pub fn init<DI, DELAY>(
    mut di: DI,
    delay: &mut DELAY,
    cold: bool,
) -> Result<mipidsi::Display<DI, ZpodIli9340, NoopReset>, mipidsi::InitError<DI::Error, core::convert::Infallible>>
where
    DI: Interface<Word = u8>,
    DELAY: DelayNs,
{
    if cold {
        di.send_command(0x01, &[]).map_err(mipidsi::InitError::Interface)?; // SWRESET
    }
    Builder::new(ZpodIli9340 { warm: !cold }, di)
        .display_size(240, 320)
        .orientation(ORIENTATION)
        .color_order(COLOR_ORDER)
        .reset_pin(NoopReset)
        .init(delay)
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use mipidsi::interface::InterfaceKind;
    use std::vec::Vec;

    #[derive(Default)]
    struct Rec {
        log: Vec<(u8, Vec<u8>)>,
        pixels: usize,
    }
    impl Interface for &mut Rec {
        type Word = u8;
        type Error = core::convert::Infallible;
        const KIND: InterfaceKind = InterfaceKind::Serial4Line;
        fn send_command(&mut self, c: u8, a: &[u8]) -> Result<(), Self::Error> {
            self.log.push((c, a.to_vec()));
            Ok(())
        }
        fn send_pixels<const N: usize>(&mut self, p: impl IntoIterator<Item = [u8; N]>) -> Result<(), Self::Error> {
            self.pixels += p.into_iter().count();
            Ok(())
        }
        fn send_repeated_pixel<const N: usize>(&mut self, _p: [u8; N], n: u32) -> Result<(), Self::Error> {
            self.pixels += n as usize;
            Ok(())
        }
    }
    struct NoDelay(u32);
    impl DelayNs for NoDelay {
        fn delay_ns(&mut self, ns: u32) {
            self.0 += ns / 1000;
        }
    }

    fn cmds(r: &Rec) -> Vec<(u8, Vec<u8>)> {
        r.log.clone()
    }

    #[test]
    fn cold_init_matches_confirmed_python_sequence() {
        let mut r = Rec::default();
        let mut d = NoDelay(0);
        let disp = cold_builder(&mut r).init(&mut d).unwrap();
        drop(disp);
        let expect: Vec<(u8, Vec<u8>)> = std::vec![
            (0x01, std::vec![]),
            (0x11, std::vec![]),
            (0x3A, std::vec![0x55]),
            (0x36, std::vec![0x08]),
            (0x20, std::vec![]),
            (0x13, std::vec![]),
            (0x29, std::vec![]),
            (0x36, std::vec![MADCTL_LANDSCAPE]),
        ];
        assert_eq!(cmds(&r), expect);
        assert!(d.0 >= 350_000, "delays {} us", d.0);
    }

    #[test]
    fn init_cold_same_as_builder() {
        let mut a = Rec::default();
        let mut b = Rec::default();
        drop(cold_builder(&mut a).init(&mut NoDelay(0)).unwrap());
        drop(init(&mut b, &mut NoDelay(0), true).unwrap());
        assert_eq!(a.log, b.log);
    }

    #[test]
    fn warm_init_has_no_reset_and_same_madctl() {
        let mut r = Rec::default();
        let mut d = NoDelay(0);
        let disp = warm_builder(&mut r).init(&mut d).unwrap();
        drop(disp);
        assert!(r.log.iter().all(|(c, _)| *c != 0x01));
        assert!(r.log.contains(&(0x36, std::vec![MADCTL_LANDSCAPE])));
        assert!(d.0 < 20_000);
    }

    #[test]
    fn window_is_landscape_320x240() {
        let mut r = Rec::default();
        let mut disp = warm_builder(&mut r).init(&mut NoDelay(0)).unwrap();
        disp.set_pixels(0, 0, 319, 239, core::iter::repeat(Rgb565::new(0, 0, 0)).take(320 * 240)).unwrap();
        drop(disp);
        // CASET 0..319, RASET 0..239, RAMWR (no offsets for this panel)
        let n = r.log.len();
        assert_eq!(r.log[n - 3], (0x2A, std::vec![0, 0, 0x01, 0x3F]));
        assert_eq!(r.log[n - 2], (0x2B, std::vec![0, 0, 0, 0xEF]));
        assert_eq!(r.log[n - 1].0, 0x2C);
        assert_eq!(r.pixels, 320 * 240);
    }
}
