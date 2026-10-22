//! Music player core (`no_std` + `alloc`): library browsing, queue,
//! now-playing screen and button handling, independent of where music comes
//! from. The Pi build plugs in an MPD backend; another platform (ESP32 with
//! a native decoder) implements [`MusicBackend`] itself.
#![no_std]
extern crate alloc;

pub mod app;
pub mod model;
pub mod view;

pub use app::{MusicApp, Outcome};
pub use model::*;
