//! Album art: MPD `albumart` (cover.jpg/png/... in the album folder), then
//! `readpicture` (embedded picture). Decoded off the UI thread, scaled to a
//! square thumbnail and cached as raw RGB565 in ~/.cache/zpod/music-art/.
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};

use zpod_mpd::Client;
use zpod_ui::image::Image565;

const MAX_BYTES: usize = 8 << 20;
const MAX_PIXELS: u64 = 25_000_000;

pub type Req = (String, String); // (uri, key)
pub type Res = (String, Option<Image565>);

fn cache_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(|| "/tmp".into());
    base.join("zpod/music-art")
}

fn fnv(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

pub fn spawn_loader(size: u32) -> (Sender<Req>, Receiver<Res>) {
    let (tx, rx) = mpsc::channel::<Req>();
    let (rtx, rrx) = mpsc::channel::<Res>();
    std::thread::Builder::new()
        .name("art".into())
        .spawn(move || {
            let dir = cache_dir();
            let _ = std::fs::create_dir_all(&dir);
            while let Ok(mut req) = rx.recv() {
                // only the newest request matters
                while let Ok(r) = rx.try_recv() {
                    req = r;
                }
                let (uri, key) = req;
                let path = dir.join(format!("{:016x}-{size}.565", fnv(&key)));
                let img = std::fs::read(&path).ok().and_then(|b| Image565::from_bytes(&b)).or_else(|| {
                    let img = fetch(&uri).and_then(|d| decode(&d, size));
                    if let Some(i) = &img {
                        let _ = std::fs::write(&path, i.to_bytes());
                    }
                    img
                });
                if rtx.send((key, img)).is_err() {
                    break;
                }
            }
        })
        .expect("spawn art thread");
    (tx, rrx)
}

fn fetch(uri: &str) -> Option<Vec<u8>> {
    let mut c = Client::connect_default().ok()?;
    if let Ok(Some(d)) = c.binary("albumart", uri, MAX_BYTES) {
        return Some(d);
    }
    c.binary("readpicture", uri, MAX_BYTES).ok().flatten()
}

/// Decode JPEG or PNG into a `size` x `size` centre-cropped thumbnail.
pub fn decode(data: &[u8], size: u32) -> Option<Image565> {
    if data.starts_with(&[0xFF, 0xD8]) {
        let mut d = jpeg_decoder::Decoder::new(data);
        d.read_info().ok()?;
        let info = d.info()?;
        if info.width as u64 * info.height as u64 > MAX_PIXELS {
            return None;
        }
        // DCT-domain downscale (1/2, 1/4, 1/8): far less CPU and RAM on the Zero
        d.scale(size as u16, size as u16).ok()?;
        let px = d.decode().ok()?;
        let info = d.info()?;
        let (w, h) = (info.width as u32, info.height as u32);
        let rgb: Vec<u8> = match info.pixel_format {
            jpeg_decoder::PixelFormat::RGB24 => px,
            jpeg_decoder::PixelFormat::L8 => px.iter().flat_map(|&l| [l, l, l]).collect(),
            jpeg_decoder::PixelFormat::L16 => px.chunks_exact(2).flat_map(|c| [c[0], c[0], c[0]]).collect(),
            jpeg_decoder::PixelFormat::CMYK32 => px
                .chunks_exact(4)
                .flat_map(|c| {
                    let k = 255 - c[3] as u32;
                    [((255 - c[0] as u32) * k / 255) as u8, ((255 - c[1] as u32) * k / 255) as u8, ((255 - c[2] as u32) * k / 255) as u8]
                })
                .collect(),
        };
        return Some(Image565::from_rgb_scaled(&rgb, w, h, 3, size, size, true));
    }
    if data.starts_with(&[0x89, b'P', b'N', b'G']) {
        let mut dec = png::Decoder::new(std::io::Cursor::new(data));
        dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let mut r = dec.read_info().ok()?;
        let (w, h) = (r.info().width, r.info().height);
        if w as u64 * h as u64 > MAX_PIXELS {
            return None;
        }
        let mut buf = vec![0u8; r.output_buffer_size()?];
        let fi = r.next_frame(&mut buf).ok()?;
        let buf = &buf[..fi.buffer_size()];
        let (rgb, bpp): (Vec<u8>, u32) = match fi.color_type {
            png::ColorType::Rgb => (buf.to_vec(), 3),
            png::ColorType::Rgba => (buf.to_vec(), 4),
            png::ColorType::Grayscale => (buf.iter().flat_map(|&l| [l, l, l]).collect(), 3),
            png::ColorType::GrayscaleAlpha => (buf.chunks_exact(2).flat_map(|c| [c[0], c[0], c[0]]).collect(), 3),
            png::ColorType::Indexed => return None,
        };
        return Some(Image565::from_rgb_scaled(&rgb, w, h, bpp, size, size, true));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_png_thumbnail() {
        let mut out = Vec::new();
        {
            let mut e = png::Encoder::new(&mut out, 200, 100);
            e.set_color(png::ColorType::Rgb);
            e.set_depth(png::BitDepth::Eight);
            let mut w = e.write_header().unwrap();
            let mut px = vec![0u8; 200 * 100 * 3];
            for (i, p) in px.chunks_exact_mut(3).enumerate() {
                if (i % 200) < 100 {
                    p[0] = 255;
                } else {
                    p[2] = 255;
                }
            }
            w.write_image_data(&px).unwrap();
        }
        let img = decode(&out, 40).unwrap();
        assert_eq!((img.w, img.h), (40, 40));
        // centre crop of a red|blue image: left half red, right half blue
        assert_eq!(img.px[0], 0xF800);
        assert_eq!(img.px[39], 0x001F);
        assert!(decode(b"not an image", 40).is_none());
    }
}
