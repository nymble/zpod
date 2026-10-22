//! RGB565 framebuffer with dirty-rectangle tracking.

use alloc::vec;
use alloc::vec::Vec;
use embedded_graphics::{
    pixelcolor::{raw::RawU16, Rgb565},
    prelude::*,
    primitives::Rectangle,
};

pub const WIDTH: usize = 320;
pub const HEIGHT: usize = 240;
const MAX_RECTS: usize = 8;

/// Inclusive pixel rectangle `x0..=x1`, `y0..=y1`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dirty {
    pub x0: u16,
    pub y0: u16,
    pub x1: u16,
    pub y1: u16,
}

impl Dirty {
    pub fn area(&self) -> u32 {
        (self.x1 - self.x0 + 1) as u32 * (self.y1 - self.y0 + 1) as u32
    }
    fn union(&self, o: &Dirty) -> Dirty {
        Dirty { x0: self.x0.min(o.x0), y0: self.y0.min(o.y0), x1: self.x1.max(o.x1), y1: self.y1.max(o.y1) }
    }
    /// Overlapping or touching (so merging costs nothing extra).
    fn touches(&self, o: &Dirty) -> bool {
        self.x0 as i32 <= o.x1 as i32 + 1
            && o.x0 as i32 <= self.x1 as i32 + 1
            && self.y0 as i32 <= o.y1 as i32 + 1
            && o.y0 as i32 <= self.y1 as i32 + 1
    }
}

pub struct Framebuffer {
    pub width: usize,
    pub height: usize,
    /// Native-endian RGB565 pixels, row major.
    pub px: Vec<u16>,
    dirty: Vec<Dirty>,
}

impl Default for Framebuffer {
    fn default() -> Self {
        Self::new(WIDTH, HEIGHT)
    }
}

impl Framebuffer {
    pub fn new(width: usize, height: usize) -> Self {
        let mut fb = Self { width, height, px: vec![0; width * height], dirty: Vec::new() };
        fb.mark_all_dirty();
        fb
    }

    pub fn mark_all_dirty(&mut self) {
        self.dirty.clear();
        self.dirty.push(Dirty { x0: 0, y0: 0, x1: self.width as u16 - 1, y1: self.height as u16 - 1 });
    }

    /// Mark a rectangle (clipped) as needing a flush.
    pub fn mark_dirty(&mut self, x0: i32, y0: i32, x1: i32, y1: i32) {
        let x0 = x0.max(0);
        let y0 = y0.max(0);
        let x1 = x1.min(self.width as i32 - 1);
        let y1 = y1.min(self.height as i32 - 1);
        if x0 > x1 || y0 > y1 {
            return;
        }
        let mut r = Dirty { x0: x0 as u16, y0: y0 as u16, x1: x1 as u16, y1: y1 as u16 };
        // Merge with everything it touches (repeat until stable).
        loop {
            let before = self.dirty.len();
            self.dirty.retain(|d| {
                if d.touches(&r) {
                    r = r.union(d);
                    false
                } else {
                    true
                }
            });
            if self.dirty.len() == before {
                break;
            }
        }
        self.dirty.push(r);
        if self.dirty.len() > MAX_RECTS {
            let all = self.dirty.iter().fold(self.dirty[0], |a, d| a.union(d));
            self.dirty.clear();
            self.dirty.push(all);
        }
    }

    pub fn is_dirty(&self) -> bool {
        !self.dirty.is_empty()
    }

    pub fn dirty_rects(&self) -> &[Dirty] {
        &self.dirty
    }

    pub fn take_dirty(&mut self) -> Vec<Dirty> {
        core::mem::take(&mut self.dirty)
    }

    #[inline]
    pub fn get(&self, x: usize, y: usize) -> u16 {
        self.px[y * self.width + x]
    }

    #[inline]
    pub fn set_raw(&mut self, x: usize, y: usize, c: u16) {
        self.px[y * self.width + x] = c;
    }

    /// Copy a block of native RGB565 pixels (`w` wide) to (x, y), clipped.
    pub fn blit(&mut self, x: i32, y: i32, w: usize, h: usize, src: &[u16]) {
        for row in 0..h as i32 {
            let ty = y + row;
            if ty < 0 || ty >= self.height as i32 {
                continue;
            }
            for col in 0..w as i32 {
                let tx = x + col;
                if tx < 0 || tx >= self.width as i32 {
                    continue;
                }
                self.px[ty as usize * self.width + tx as usize] = src[row as usize * w + col as usize];
            }
        }
        self.mark_dirty(x, y, x + w as i32 - 1, y + h as i32 - 1);
    }

    /// Pixels as RGB888 (for PNG screenshots).
    pub fn to_rgb888(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.px.len() * 3);
        for &p in &self.px {
            let (r, g, b) = rgb565_to_888(p);
            out.extend_from_slice(&[r, g, b]);
        }
        out
    }
}

#[inline]
pub fn rgb565_to_888(p: u16) -> (u8, u8, u8) {
    let r = ((p >> 11) & 0x1f) as u8;
    let g = ((p >> 5) & 0x3f) as u8;
    let b = (p & 0x1f) as u8;
    ((r << 3) | (r >> 2), (g << 2) | (g >> 4), (b << 3) | (b >> 2))
}

#[inline]
pub const fn rgb888_to_565(r: u8, g: u8, b: u8) -> u16 {
    ((r as u16 & 0xf8) << 8) | ((g as u16 & 0xfc) << 3) | (b as u16 >> 3)
}

impl OriginDimensions for Framebuffer {
    fn size(&self) -> Size {
        Size::new(self.width as u32, self.height as u32)
    }
}

impl DrawTarget for Framebuffer {
    type Color = Rgb565;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        let (w, h) = (self.width as i32, self.height as i32);
        let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for Pixel(p, c) in pixels {
            if p.x >= 0 && p.y >= 0 && p.x < w && p.y < h {
                self.px[(p.y * w + p.x) as usize] = RawU16::from(c).into_inner();
                x0 = x0.min(p.x);
                y0 = y0.min(p.y);
                x1 = x1.max(p.x);
                y1 = y1.max(p.y);
            }
        }
        if x0 <= x1 {
            self.mark_dirty(x0, y0, x1, y1);
        }
        Ok(())
    }

    fn fill_solid(&mut self, area: &Rectangle, color: Self::Color) -> Result<(), Self::Error> {
        let a = area.intersection(&self.bounding_box());
        if a.size.width == 0 || a.size.height == 0 {
            return Ok(());
        }
        let c = RawU16::from(color).into_inner();
        let (x, y) = (a.top_left.x as usize, a.top_left.y as usize);
        let (aw, ah) = (a.size.width as usize, a.size.height as usize);
        for row in y..y + ah {
            self.px[row * self.width + x..row * self.width + x + aw].fill(c);
        }
        self.mark_dirty(x as i32, y as i32, (x + aw) as i32 - 1, (y + ah) as i32 - 1);
        Ok(())
    }

    fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
        self.px.fill(RawU16::from(color).into_inner());
        self.mark_all_dirty();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};

    #[test]
    fn dirty_merges_touching() {
        let mut fb = Framebuffer::new(320, 240);
        fb.take_dirty();
        fb.mark_dirty(0, 0, 9, 9);
        fb.mark_dirty(10, 0, 19, 9);
        assert_eq!(fb.dirty_rects(), &[Dirty { x0: 0, y0: 0, x1: 19, y1: 9 }]);
        fb.mark_dirty(100, 100, 110, 110);
        assert_eq!(fb.dirty_rects().len(), 2);
    }

    #[test]
    fn dirty_clips_and_caps() {
        let mut fb = Framebuffer::new(320, 240);
        fb.take_dirty();
        fb.mark_dirty(-5, -5, 1000, 3);
        assert_eq!(fb.dirty_rects(), &[Dirty { x0: 0, y0: 0, x1: 319, y1: 3 }]);
        fb.take_dirty();
        for i in 0..20 {
            fb.mark_dirty(i * 15, i * 11, i * 15 + 2, i * 11 + 2);
        }
        assert!(fb.dirty_rects().len() <= MAX_RECTS);
    }

    #[test]
    fn draw_marks_bbox() {
        let mut fb = Framebuffer::new(320, 240);
        fb.take_dirty();
        Rectangle::new(Point::new(10, 20), Size::new(5, 6))
            .into_styled(PrimitiveStyle::with_fill(Rgb565::RED))
            .draw(&mut fb)
            .unwrap();
        assert_eq!(fb.dirty_rects(), &[Dirty { x0: 10, y0: 20, x1: 14, y1: 25 }]);
        assert_eq!(fb.get(10, 20), 0xF800);
    }

    #[test]
    fn color_roundtrip() {
        assert_eq!(rgb888_to_565(255, 0, 0), 0xF800);
        assert_eq!(rgb565_to_888(0xFFFF), (255, 255, 255));
    }
}
