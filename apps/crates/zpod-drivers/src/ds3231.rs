//! DS3231 RTC (ZPOD: I2C-1 address 0x68). 24-hour mode, UTC recommended.
//!
//! On the Pi the kernel `rtc-ds1307` driver (dtoverlay=i2c-rtc,ds3231) plus
//! `hwclock` is the preferred path; this driver exists for platforms without
//! a kernel (ESP32) and for diagnostics.

use embedded_hal::i2c::I2c;
use zpod_core::DateTime;

pub const ADDR: u8 = 0x68;
const REG_TIME: u8 = 0x00;
const REG_STATUS: u8 = 0x0F;
const REG_TEMP: u8 = 0x11;
const OSF: u8 = 0x80;

#[derive(Debug, PartialEq, Eq)]
pub enum Error<E> {
    I2c(E),
    /// Oscillator stopped flag set: time is not valid (never set / battery lost).
    NotSet,
    Invalid,
}

fn bcd2bin(v: u8) -> u8 {
    (v >> 4) * 10 + (v & 0x0f)
}
fn bin2bcd(v: u8) -> u8 {
    ((v / 10) << 4) | (v % 10)
}

pub struct Ds3231<I2C> {
    i2c: I2C,
}

impl<I2C: I2c> Ds3231<I2C> {
    pub fn new(i2c: I2C) -> Self {
        Self { i2c }
    }

    pub fn time_valid(&mut self) -> Result<bool, I2C::Error> {
        let mut s = [0u8];
        self.i2c.write_read(ADDR, &[REG_STATUS], &mut s)?;
        Ok(s[0] & OSF == 0)
    }

    pub fn read(&mut self) -> Result<DateTime, Error<I2C::Error>> {
        if !self.time_valid().map_err(Error::I2c)? {
            return Err(Error::NotSet);
        }
        let mut r = [0u8; 7];
        self.i2c.write_read(ADDR, &[REG_TIME], &mut r).map_err(Error::I2c)?;
        let hour = if r[2] & 0x40 != 0 {
            // 12 h mode
            let h = bcd2bin(r[2] & 0x1f) % 12;
            if r[2] & 0x20 != 0 { h + 12 } else { h }
        } else {
            bcd2bin(r[2] & 0x3f)
        };
        let century = if r[5] & 0x80 != 0 { 2100 } else { 2000 };
        let dt = DateTime {
            second: bcd2bin(r[0] & 0x7f),
            minute: bcd2bin(r[1] & 0x7f),
            hour,
            weekday: (r[3] & 0x07) % 7, // stored 1..7, 7 = Sunday -> 0
            day: bcd2bin(r[4] & 0x3f),
            month: bcd2bin(r[5] & 0x1f),
            year: century + bcd2bin(r[6]) as i32,
        };
        if dt.month == 0 || dt.month > 12 || dt.day == 0 || dt.day > 31 || dt.hour > 23 || dt.minute > 59 {
            return Err(Error::Invalid);
        }
        Ok(dt)
    }

    /// Set the time (24 h mode) and clear the oscillator-stopped flag.
    pub fn set(&mut self, t: &DateTime) -> Result<(), Error<I2C::Error>> {
        if !(2000..2200).contains(&t.year) {
            return Err(Error::Invalid);
        }
        let c = if t.year >= 2100 { 0x80 } else { 0 };
        let wd = if t.weekday == 0 { 7 } else { t.weekday };
        let buf = [
            REG_TIME,
            bin2bcd(t.second),
            bin2bcd(t.minute),
            bin2bcd(t.hour),
            wd,
            bin2bcd(t.day),
            bin2bcd(t.month) | c,
            bin2bcd((t.year % 100) as u8),
        ];
        self.i2c.write(ADDR, &buf).map_err(Error::I2c)?;
        let mut s = [0u8];
        self.i2c.write_read(ADDR, &[REG_STATUS], &mut s).map_err(Error::I2c)?;
        self.i2c.write(ADDR, &[REG_STATUS, s[0] & !OSF]).map_err(Error::I2c)?;
        Ok(())
    }

    /// Die temperature in C (0.25 C resolution).
    pub fn temperature(&mut self) -> Result<f32, I2C::Error> {
        let mut r = [0u8; 2];
        self.i2c.write_read(ADDR, &[REG_TEMP], &mut r)?;
        Ok(r[0] as i8 as f32 + (r[1] >> 6) as f32 * 0.25)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::MockI2c;

    #[test]
    fn set_then_read_roundtrip() {
        let mut m = MockI2c::new(ADDR);
        m.regs[REG_STATUS as usize] = OSF; // power-on state: not set
        let mut rtc = Ds3231::new(m);
        assert_eq!(rtc.read(), Err(Error::NotSet));
        let t = DateTime::from_unix(1_792_258_500);
        rtc.set(&t).unwrap();
        assert_eq!(rtc.read().unwrap(), t);
    }

    #[test]
    fn twelve_hour_mode() {
        let mut m = MockI2c::new(ADDR);
        m.regs[0..7].copy_from_slice(&[0x30, 0x15, 0x40 | 0x20 | 0x11, 3, 0x17, 0x10, 0x26]);
        let t = Ds3231::new(m).read().unwrap();
        assert_eq!((t.hour, t.minute, t.second, t.year), (23, 15, 30, 2026));
    }

    #[test]
    fn temperature() {
        let mut m = MockI2c::new(ADDR);
        m.regs[0x11] = 25;
        m.regs[0x12] = 0x40;
        assert_eq!(Ds3231::new(m).temperature().unwrap(), 25.25);
    }
}
