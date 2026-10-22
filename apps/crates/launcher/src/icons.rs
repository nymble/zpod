//! App icons: 32x32 PNG from the app dir, pre-composited onto the two row
//! backgrounds (normal / selected) so alpha edges look right on RGB565.

use embedded_graphics::pixelcolor::{raw::RawU16, Rgb565};
use embedded_graphics::prelude::*;
use zpod_ui::image::Image565;
use zpod_ui::theme::{BG, SEL};

pub const SIZE: u32 = 32;

pub struct Icon {
    pub normal: Image565,
    pub selected: Image565,
}

pub static DEFAULT_PNG: &[u8] = include_bytes!("../../../assets/icons/default.png");
pub static SYSTEM_PNG: &[u8] = include_bytes!("../../../assets/icons/system.png");

/// Decode to RGBA8888 (w, h, pixels).
pub fn decode_png(data: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let mut dec = png::Decoder::new(std::io::Cursor::new(data));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16 | png::Transformations::ALPHA);
    let mut r = dec.read_info().ok()?;
    let (w, h) = (r.info().width, r.info().height);
    if w == 0 || h == 0 || w > 512 || h > 512 {
        return None;
    }
    let mut buf = vec![0; r.output_buffer_size()?];
    let fi = r.next_frame(&mut buf).ok()?;
    let buf = &buf[..fi.buffer_size()];
    let rgba = match fi.color_type {
        png::ColorType::Rgba => buf.to_vec(),
        png::ColorType::Rgb => buf.chunks_exact(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => buf.chunks_exact(2).flat_map(|c| [c[0], c[0], c[0], c[1]]).collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|&l| [l, l, l, 255]).collect(),
        png::ColorType::Indexed => return None,
    };
    Some((w, h, rgba))
}

fn composite(w: u32, h: u32, rgba: &[u8], bg: Rgb565) -> Image565 {
    let (br, bgc, bb) = {
        let r = RawU16::from(bg).into_inner();
        let (r8, g8, b8) = zpod_ui::fb::rgb565_to_888(r);
        (r8 as u32, g8 as u32, b8 as u32)
    };
    // nearest/box scale to SIZE first (in RGBA), then blend
    let mut out = Image565::new(SIZE, SIZE, 0);
    for ty in 0..SIZE {
        for tx in 0..SIZE {
            let (xa, xb) = (tx * w / SIZE, ((tx + 1) * w / SIZE).max(tx * w / SIZE + 1));
            let (ya, yb) = (ty * h / SIZE, ((ty + 1) * h / SIZE).max(ty * h / SIZE + 1));
            let (mut r, mut g, mut b, mut a, mut n) = (0u32, 0u32, 0u32, 0u32, 0u32);
            for y in ya..yb.min(h) {
                for x in xa..xb.min(w) {
                    let i = ((y * w + x) * 4) as usize;
                    let al = rgba[i + 3] as u32;
                    r += rgba[i] as u32 * al;
                    g += rgba[i + 1] as u32 * al;
                    b += rgba[i + 2] as u32 * al;
                    a += al;
                    n += 1;
                }
            }
            let n = n.max(1);
            let alpha = a / n; // 0..255
            let (r, g, b) = if a > 0 { (r / a, g / a, b / a) } else { (0, 0, 0) };
            let mix = |c: u32, k: u32| (c * alpha + k * (255 - alpha)) / 255;
            let px = zpod_ui::fb::rgb888_to_565(mix(r, br) as u8, mix(g, bgc) as u8, mix(b, bb) as u8);
            out.px[(ty * SIZE + tx) as usize] = px;
        }
    }
    out
}

pub fn from_png(data: &[u8]) -> Option<Icon> {
    let (w, h, rgba) = decode_png(data)?;
    Some(Icon { normal: composite(w, h, &rgba, BG), selected: composite(w, h, &rgba, SEL) })
}

pub fn load(path: Option<&std::path::Path>) -> Icon {
    path.and_then(|p| std::fs::read(p).ok())
        .and_then(|d| from_png(&d))
        .unwrap_or_else(|| from_png(DEFAULT_PNG).expect("embedded icon"))
}

/// Draw an icon scaled up 2x (launch screen).
pub fn draw_2x<D: DrawTarget<Color = Rgb565>>(d: &mut D, img: &Image565, x: i32, y: i32) {
    let w = img.w as i32;
    let px = (0..img.h as i32 * 2).flat_map(|yy| {
        (0..w * 2).map(move |xx| ((xx, yy), img.px[((yy / 2) * w + xx / 2) as usize]))
    });
    let _ = d.draw_iter(px.map(|((xx, yy), c)| Pixel(Point::new(x + xx, y + yy), Rgb565::from(RawU16::new(c)))));
}

#[cfg(test)]
mod tests {
    use embedded_graphics::prelude::RawData;
    #[test]
    fn embedded_icons_decode() {
        let i = super::from_png(super::SYSTEM_PNG).unwrap();
        assert_eq!((i.normal.w, i.normal.h), (32, 32));
        // corners are transparent -> background colour
        assert_eq!(i.normal.px[0], embedded_graphics::pixelcolor::raw::RawU16::from(zpod_ui::theme::BG).into_inner());
        assert_ne!(i.normal.px[0], i.selected.px[0]);
        assert!(super::from_png(b"garbage").is_none());
    }
}
