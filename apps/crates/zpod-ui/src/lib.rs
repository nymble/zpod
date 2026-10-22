//! ZPOD UI kit (`no_std` + `alloc`). Everything draws onto any
//! `embedded-graphics` `DrawTarget<Color = Rgb565>`; [`fb::Framebuffer`] is
//! the usual target (dirty-rect tracked, flushed by the platform layer).
#![no_std]
extern crate alloc;

pub mod fb;
pub mod image;
pub mod status;
pub mod text;
pub mod theme;
pub mod widgets;

pub use fb::Framebuffer;
pub use text::Font;

use embedded_graphics::{pixelcolor::Rgb565, prelude::*, primitives::Rectangle};

/// Shorthand for the draw targets used by every widget.
pub trait Canvas: DrawTarget<Color = Rgb565> {}
impl<T: DrawTarget<Color = Rgb565>> Canvas for T {}

pub fn rect(x: i32, y: i32, w: u32, h: u32) -> Rectangle {
    Rectangle::new(Point::new(x, y), Size::new(w, h))
}

pub fn fill<D: Canvas>(d: &mut D, x: i32, y: i32, w: u32, h: u32, c: Rgb565) {
    let _ = d.fill_solid(&rect(x, y, w, h), c);
}
