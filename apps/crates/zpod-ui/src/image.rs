//! Small RGB565 images (album art, icons) with nearest/box scaling.
use alloc::vec;
use alloc::vec::Vec;
use embedded_graphics::{pixelcolor::raw::RawU16, pixelcolor::Rgb565};

use crate::Canvas;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image565 {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u16>,
}

impl Image565 {
    pub fn new(w: u32, h: u32, fill: u16) -> Self {
        Self { w, h, px: vec![fill; (w * h) as usize] }
    }

    /// From packed RGB888 (or RGBA8888 when `stride_px` = 4) by box-filter
    /// downscaling into `tw` x `th` (aspect is cropped to square-ish centre
    /// when `crop` is true).
    pub fn from_rgb_scaled(src: &[u8], sw: u32, sh: u32, bpp: u32, tw: u32, th: u32, crop: bool) -> Self {
        let (mut x0, mut y0, mut cw, mut ch) = (0, 0, sw, sh);
        if crop {
            // crop source to target aspect
            if sw * th > sh * tw {
                cw = sh * tw / th;
                x0 = (sw - cw) / 2;
            } else {
                ch = sw * th / tw;
                y0 = (sh - ch) / 2;
            }
        }
        let mut out = Vec::with_capacity((tw * th) as usize);
        for ty in 0..th {
            let ya = y0 + ty * ch / th;
            let yb = (y0 + (ty + 1) * ch / th).max(ya + 1).min(sh);
            for tx in 0..tw {
                let xa = x0 + tx * cw / tw;
                let xb = (x0 + (tx + 1) * cw / tw).max(xa + 1).min(sw);
                let (mut r, mut g, mut b, mut n) = (0u32, 0u32, 0u32, 0u32);
                // sample at most 4x4 points per output pixel to bound cost
                let sy = ((yb - ya) / 4).max(1);
                let sx = ((xb - xa) / 4).max(1);
                let mut y = ya;
                while y < yb {
                    let mut x = xa;
                    while x < xb {
                        let i = ((y * sw + x) * bpp) as usize;
                        r += src[i] as u32;
                        g += src[i + 1] as u32;
                        b += src[i + 2] as u32;
                        n += 1;
                        x += sx;
                    }
                    y += sy;
                }
                let n = n.max(1);
                out.push(rgb888(r / n, g / n, b / n));
            }
        }
        Self { w: tw, h: th, px: out }
    }

    pub fn draw<D: Canvas>(&self, d: &mut D, x: i32, y: i32) {
        let area = crate::rect(x, y, self.w, self.h);
        let _ = d.fill_contiguous(&area, self.px.iter().map(|&p| Rgb565::from(RawU16::new(p))));
    }

    /// Raw little-endian bytes (cache files).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut v = Vec::with_capacity(8 + self.px.len() * 2);
        v.extend_from_slice(&self.w.to_le_bytes());
        v.extend_from_slice(&self.h.to_le_bytes());
        for p in &self.px {
            v.extend_from_slice(&p.to_le_bytes());
        }
        v
    }

    pub fn from_bytes(b: &[u8]) -> Option<Self> {
        if b.len() < 8 {
            return None;
        }
        let w = u32::from_le_bytes(b[0..4].try_into().ok()?);
        let h = u32::from_le_bytes(b[4..8].try_into().ok()?);
        if w == 0 || h == 0 || w > 1024 || h > 1024 || b.len() != 8 + (w * h * 2) as usize {
            return None;
        }
        let px = b[8..].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        Some(Self { w, h, px })
    }
}

#[inline]
fn rgb888(r: u32, g: u32, b: u32) -> u16 {
    (((r & 0xf8) << 8) | ((g & 0xfc) << 3) | (b >> 3)) as u16
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scale_and_roundtrip() {
        // 4x2 image: left half red, right half blue
        let mut src = vec![];
        for _y in 0..2 {
            for x in 0..4 {
                if x < 2 { src.extend_from_slice(&[255, 0, 0]) } else { src.extend_from_slice(&[0, 0, 255]) }
            }
        }
        let img = Image565::from_rgb_scaled(&src, 4, 2, 3, 2, 1, false);
        assert_eq!(img.px, vec![0xF800, 0x001F]);
        let sq = Image565::from_rgb_scaled(&src, 4, 2, 3, 2, 2, true);
        assert_eq!(sq.w, 2);
        assert_eq!(Image565::from_bytes(&img.to_bytes()).unwrap(), img);
    }
}
