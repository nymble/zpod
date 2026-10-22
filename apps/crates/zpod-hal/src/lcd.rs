//! ZPOD panel on Linux: mipidsi (custom ZPOD model) over spidev + gpio-cdev.

use std::io;

use embedded_graphics::pixelcolor::{raw::RawU16, Rgb565};
use embedded_hal::digital::OutputPin;
use linux_embedded_hal::{
    gpio_cdev::{chips, Chip, LineRequestFlags},
    spidev::{SpiModeFlags, SpidevOptions},
    CdevPin, Delay, SpidevDevice,
};
use mipidsi::interface::{Interface, SpiInterface};
use zpod_panel::{NoopReset, ZpodIli9340};
use zpod_ui::fb::Framebuffer;

pub const SPI_DEV: &str = "/dev/spidev0.0";
pub const SPI_HZ: u32 = 32_000_000;
pub const PIN_DC: u32 = 25;
pub const PIN_BL: u32 = 27;
/// Headphone amp mute (owned by config.txt `gpio=14=op,dl`). Never touched.
pub const AMP_MUTE: u32 = 14;
const _: () = assert!(PIN_DC != AMP_MUTE && PIN_BL != AMP_MUTE);

/// Marker in /run written once the panel had its full (cold) init this boot,
/// so later processes do the flash-free warm init.
pub const INIT_MARK: &str = "/run/zpod/lcd-initialised";

/// DC line: gpio-cdev (normal) or /dev/gpiomem (fallback when the line is
/// busy, same as zpod_lcd.py: `pinctrl set 25 op dh` + GPSET0/GPCLR0).
pub enum DcPin {
    Cdev(CdevPin),
    Mem(GpioMem),
}

impl embedded_hal::digital::ErrorType for DcPin {
    type Error = embedded_hal::digital::ErrorKind;
}

impl OutputPin for DcPin {
    fn set_low(&mut self) -> Result<(), Self::Error> {
        match self {
            DcPin::Cdev(p) => p.set_low().map_err(|_| embedded_hal::digital::ErrorKind::Other),
            DcPin::Mem(m) => {
                m.write(PIN_DC, false);
                Ok(())
            }
        }
    }
    fn set_high(&mut self) -> Result<(), Self::Error> {
        match self {
            DcPin::Cdev(p) => p.set_high().map_err(|_| embedded_hal::digital::ErrorKind::Other),
            DcPin::Mem(m) => {
                m.write(PIN_DC, true);
                Ok(())
            }
        }
    }
}

/// /dev/gpiomem mapping (BCM283x GPIO block), write-only use for one pin.
pub struct GpioMem {
    base: *mut u32,
}
unsafe impl Send for GpioMem {}

impl GpioMem {
    pub fn open() -> io::Result<Self> {
        use std::os::fd::AsRawFd;
        let f = std::fs::OpenOptions::new().read(true).write(true).open("/dev/gpiomem")?;
        let p = unsafe {
            libc::mmap(std::ptr::null_mut(), 4096, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, f.as_raw_fd(), 0)
        };
        if p == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }
        Ok(GpioMem { base: p as *mut u32 })
    }
    pub fn write(&mut self, pin: u32, high: bool) {
        // GPSET0 = 0x1C, GPCLR0 = 0x28
        let off = if high { 0x1C / 4 } else { 0x28 / 4 };
        unsafe { std::ptr::write_volatile(self.base.add(off), 1 << pin) };
    }
}

impl Drop for GpioMem {
    fn drop(&mut self) {
        unsafe { libc::munmap(self.base as *mut libc::c_void, 4096) };
    }
}

fn soc_chip() -> io::Result<Chip> {
    let mut first = None;
    if let Ok(it) = chips() {
        for c in it.flatten() {
            let label = c.label().to_string();
            if label.contains("bcm2835") || label.contains("bcm2711") || label.contains("rp1") {
                return Ok(c);
            }
            if first.is_none() {
                first = Some(c);
            }
        }
    }
    first.ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no GPIO chip"))
}

fn ioerr<E: std::fmt::Debug>(what: &str) -> impl FnOnce(E) -> io::Error + '_ {
    move |e| io::Error::other(format!("{what}: {e:?}"))
}

type Di = SpiInterface<'static, SpidevDevice, DcPin>;
type Disp = mipidsi::Display<Di, ZpodIli9340, NoopReset>;

pub struct Lcd {
    disp: Option<Disp>,
    buf: *mut [u8],
    _bl: Option<CdevPin>,
    /// What the panel currently shows (to skip unchanged pixels).
    shadow: Vec<u16>,
    shadow_valid: bool,
    pub bytes_sent: u64,
}

impl Lcd {
    /// Open spidev + GPIOs and initialise. `cold`: SWRESET + full sequence;
    /// `None`: cold only if no process has initialised the panel this boot.
    pub fn open(cold: Option<bool>) -> io::Result<Self> {
        let mut spi = SpidevDevice::open(SPI_DEV).map_err(ioerr("open spidev0.0"))?;
        spi.configure(&SpidevOptions::new().bits_per_word(8).max_speed_hz(SPI_HZ).mode(SpiModeFlags::SPI_MODE_0).build())?;

        let mut chip = soc_chip()?;
        let dc = match chip
            .get_line(PIN_DC)
            .and_then(|l| l.request(LineRequestFlags::OUTPUT, 1, "zpod-lcd-dc"))
        {
            Ok(h) => DcPin::Cdev(CdevPin::new(h).map_err(ioerr("dc pin"))?),
            Err(_) => {
                let _ = std::process::Command::new("pinctrl").args(["set", &PIN_DC.to_string(), "op", "dh"]).status();
                DcPin::Mem(GpioMem::open()?)
            }
        };
        let bl = chip
            .get_line(PIN_BL)
            .and_then(|l| l.request(LineRequestFlags::OUTPUT, 1, "zpod-lcd-bl"))
            .ok()
            .and_then(|h| CdevPin::new(h).ok());

        // spidev rejects transfers larger than its bufsiz (default 4096).
        let bufsiz = std::fs::read_to_string("/sys/module/spidev/parameters/bufsiz")
            .ok()
            .and_then(|s| s.trim().parse::<usize>().ok())
            .unwrap_or(4096)
            .clamp(512, 65536);
        let buf: &'static mut [u8] = Box::leak(vec![0u8; bufsiz].into_boxed_slice());
        let raw: *mut [u8] = buf;
        let di = SpiInterface::new(spi, dc, unsafe { &mut *raw });

        let cold = cold.unwrap_or_else(|| !std::path::Path::new(INIT_MARK).exists());
        let disp = zpod_panel::init(di, &mut Delay, cold).map_err(ioerr("panel init"))?;
        if cold {
            let _ = std::fs::create_dir_all("/run/zpod");
            let _ = std::fs::write(INIT_MARK, b"1\n");
        }
        Ok(Lcd {
            disp: Some(disp),
            buf: raw,
            _bl: bl,
            shadow: vec![0; 320 * 240],
            shadow_valid: false,
            bytes_sent: 0,
        })
    }

    fn di(&mut self) -> &mut Di {
        // SAFETY: only used for raw DCS commands while no pixel write is in progress.
        unsafe { self.disp.as_mut().unwrap().dcs() }
    }

    pub fn cmd(&mut self, c: u8, args: &[u8]) {
        let _ = self.di().send_command(c, args);
    }

    /// Force the next flush to send every dirty pixel (panel content unknown).
    pub fn invalidate(&mut self) {
        self.shadow_valid = false;
    }

    /// Send the framebuffer's dirty rectangles, trimmed to pixels that differ
    /// from what the panel already shows.
    pub fn flush(&mut self, fb: &mut Framebuffer) {
        let w = fb.width;
        for r in fb.take_dirty() {
            let (mut x0, mut y0, mut x1, mut y1) = (r.x0 as usize, r.y0 as usize, r.x1 as usize, r.y1 as usize);
            if self.shadow_valid {
                let changed = |y: usize, a: usize, b: usize, sh: &[u16]| fb.px[y * w + a..=y * w + b] != sh[y * w + a..=y * w + b];
                while y0 <= y1 && !changed(y0, x0, x1, &self.shadow) {
                    y0 += 1;
                }
                if y0 > y1 {
                    continue;
                }
                while y1 > y0 && !changed(y1, x0, x1, &self.shadow) {
                    y1 -= 1;
                }
                let col_changed = |x: usize, sh: &[u16]| (y0..=y1).any(|y| fb.px[y * w + x] != sh[y * w + x]);
                while x0 < x1 && !col_changed(x0, &self.shadow) {
                    x0 += 1;
                }
                while x1 > x0 && !col_changed(x1, &self.shadow) {
                    x1 -= 1;
                }
            }
            let px = &fb.px;
            let it = (y0..=y1).flat_map(move |y| (x0..=x1).map(move |x| Rgb565::from(RawU16::new(px[y * w + x]))));
            if let Some(d) = self.disp.as_mut() {
                let _ = d.set_pixels(x0 as u16, y0 as u16, x1 as u16, y1 as u16, it);
            }
            for y in y0..=y1 {
                self.shadow[y * w + x0..=y * w + x1].copy_from_slice(&fb.px[y * w + x0..=y * w + x1]);
            }
            self.bytes_sent += ((x1 - x0 + 1) * (y1 - y0 + 1) * 2) as u64;
            // The shadow is trustworthy once a full frame has gone out.
            if !self.shadow_valid && x0 == 0 && y0 == 0 && x1 == w - 1 && y1 == fb.height - 1 {
                self.shadow_valid = true;
            }
        }
    }

    pub fn display_on(&mut self, on: bool) {
        self.cmd(if on { 0x29 } else { 0x28 }, &[]);
    }

    pub fn backlight(&mut self, on: bool) {
        if let Some(p) = self._bl.as_mut() {
            let _ = if on { p.set_high() } else { p.set_low() };
        }
    }

    /// Leave the panel dark: black, DISPOFF, SLPIN, backlight low (M3 behaviour).
    pub fn blank_off(&mut self) {
        if let Some(d) = self.disp.as_mut() {
            let _ = d.set_pixels(0, 0, 319, 239, core::iter::repeat(Rgb565::new(0, 0, 0)).take(320 * 240));
        }
        self.cmd(0x28, &[]);
        self.cmd(0x10, &[]);
        std::thread::sleep(std::time::Duration::from_millis(10));
        self.backlight(false);
        let _ = std::fs::remove_file(INIT_MARK); // next user must cold-init
    }
}

impl Drop for Lcd {
    fn drop(&mut self) {
        // Drop the display (releases spidev + DC line) before freeing its buffer.
        self.disp.take();
        unsafe { drop(Box::from_raw(self.buf)) };
    }
}
