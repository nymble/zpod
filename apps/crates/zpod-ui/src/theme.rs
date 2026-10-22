//! ZPOD colour theme (dark, high contrast for a small TFT).
use embedded_graphics::pixelcolor::Rgb565;

pub const fn rgb(r: u8, g: u8, b: u8) -> Rgb565 {
    Rgb565::new(r >> 3, g >> 2, b >> 3)
}

pub const BG: Rgb565 = rgb(8, 10, 24);
pub const PANEL: Rgb565 = rgb(22, 26, 52);
pub const BAR: Rgb565 = rgb(16, 18, 40);
pub const FG: Rgb565 = rgb(236, 236, 240);
pub const DIM: Rgb565 = rgb(140, 144, 168);
pub const FAINT: Rgb565 = rgb(70, 74, 100);
pub const ACCENT: Rgb565 = rgb(255, 196, 0);
pub const SEL: Rgb565 = rgb(0, 96, 176);
pub const OK: Rgb565 = rgb(0, 216, 120);
pub const WARN: Rgb565 = rgb(255, 84, 64);
pub const CYAN: Rgb565 = rgb(64, 200, 255);
pub const BLACK: Rgb565 = rgb(0, 0, 0);
pub const WHITE: Rgb565 = rgb(255, 255, 255);

/// Status bar height in pixels (every app keeps it at the top).
pub const STATUS_H: i32 = 20;
