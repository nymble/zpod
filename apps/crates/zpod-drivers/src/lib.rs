//! I2C chip drivers written against `embedded-hal` 1.0 so they run on Linux
//! (`linux-embedded-hal::I2cdev` on /dev/i2c-1) and on ESP32 unchanged.
#![no_std]

pub mod ds3231;
pub mod max17048;

#[cfg(test)]
pub(crate) mod mock {
    use embedded_hal::i2c::{ErrorKind, ErrorType, I2c, Operation};

    /// Register-file I2C mock: one device, 256 byte registers, auto-increment.
    pub struct MockI2c {
        pub addr: u8,
        pub regs: [u8; 256],
        ptr: u8,
    }
    impl MockI2c {
        pub fn new(addr: u8) -> Self {
            Self { addr, regs: [0; 256], ptr: 0 }
        }
    }
    impl ErrorType for MockI2c {
        type Error = ErrorKind;
    }
    impl I2c for MockI2c {
        fn transaction(&mut self, address: u8, ops: &mut [Operation<'_>]) -> Result<(), ErrorKind> {
            if address != self.addr {
                return Err(ErrorKind::NoAcknowledge(embedded_hal::i2c::NoAcknowledgeSource::Address));
            }
            for op in ops {
                match op {
                    Operation::Write(w) => {
                        if let Some((&p, rest)) = w.split_first() {
                            self.ptr = p;
                            for &b in rest {
                                self.regs[self.ptr as usize] = b;
                                self.ptr = self.ptr.wrapping_add(1);
                            }
                        }
                    }
                    Operation::Read(r) => {
                        for b in r.iter_mut() {
                            *b = self.regs[self.ptr as usize];
                            self.ptr = self.ptr.wrapping_add(1);
                        }
                    }
                }
            }
            Ok(())
        }
    }
}
