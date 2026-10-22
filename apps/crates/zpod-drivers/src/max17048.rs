//! MAX17048 single-cell fuel gauge (ZPOD: I2C-1 address 0x36).

use embedded_hal::i2c::I2c;

pub const ADDR: u8 = 0x36;
const REG_VCELL: u8 = 0x02;
const REG_SOC: u8 = 0x04;
const REG_VERSION: u8 = 0x08;
const REG_CRATE: u8 = 0x16;

pub struct Max17048<I2C> {
    i2c: I2C,
    addr: u8,
}

impl<I2C: I2c> Max17048<I2C> {
    pub fn new(i2c: I2C) -> Self {
        Self { i2c, addr: ADDR }
    }

    fn read16(&mut self, reg: u8) -> Result<u16, I2C::Error> {
        let mut b = [0u8; 2];
        self.i2c.write_read(self.addr, &[reg], &mut b)?;
        Ok(u16::from_be_bytes(b))
    }

    /// State of charge in percent (1/256 % resolution), clamped 0..=100.
    pub fn soc_percent(&mut self) -> Result<f32, I2C::Error> {
        let v = self.read16(REG_SOC)?;
        Ok(((v >> 8) as f32 + (v & 0xff) as f32 / 256.0).clamp(0.0, 100.0))
    }

    /// Cell voltage (78.125 uV/LSB).
    pub fn vcell(&mut self) -> Result<f32, I2C::Error> {
        Ok(self.read16(REG_VCELL)? as f32 * 78.125e-6)
    }

    /// Charge/discharge rate in %/hour (0.208 %/hr per LSB, signed).
    /// Positive = charging. Present on MAX17048/9 (not MAX17043).
    pub fn crate_pct_per_hour(&mut self) -> Result<f32, I2C::Error> {
        Ok(self.read16(REG_CRATE)? as i16 as f32 * 0.208)
    }

    pub fn version(&mut self) -> Result<u16, I2C::Error> {
        self.read16(REG_VERSION)
    }

    pub fn release(self) -> I2C {
        self.i2c
    }
}

impl<I2C: I2c> zpod_core::Battery for Max17048<I2C> {
    fn read(&mut self) -> Option<(f32, f32)> {
        Some((self.soc_percent().ok()?, self.vcell().ok()?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::MockI2c;

    #[test]
    fn reads_soc_and_voltage() {
        let mut m = MockI2c::new(ADDR);
        m.regs[4] = 87;
        m.regs[5] = 128; // 87.5 %
        m.regs[2] = 0xC8;
        m.regs[3] = 0x00; // 0xC800 * 78.125uV = 4.0 V
        m.regs[0x16] = 0xFF;
        m.regs[0x17] = 0xF6; // -10 -> -2.08 %/h
        let mut g = Max17048::new(m);
        assert!((g.soc_percent().unwrap() - 87.5).abs() < 1e-4);
        assert!((g.vcell().unwrap() - 4.0).abs() < 1e-3);
        assert!((g.crate_pct_per_hour().unwrap() + 2.08).abs() < 1e-3);
    }

    #[test]
    fn soc_clamped() {
        let mut m = MockI2c::new(ADDR);
        m.regs[4] = 103;
        assert_eq!(Max17048::new(m).soc_percent().unwrap(), 100.0);
    }
}
