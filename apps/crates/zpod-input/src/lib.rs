//! Normalized input for ZPOD apps (spec: docs/USB-HOST-PLAN.md §4.1, §4.7,
//! Contrail-proposed begin/update/end contract).
//!
//! * [`event`]: the normalized model. Every event carries a monotonic
//!   timestamp (µs, `CLOCK_MONOTONIC`), a source id (one per device), a
//!   contact id (one per key / button / pointer) and a phase
//!   (`Begin` / `Update` / `End`). Generators and apps only ever see these,
//!   never raw HID reports or `input_event`s.
//! * [`normalize`]: pure state machine raw evdev triples -> [`InputEvent`]s
//!   (unit-tested on any host).
//! * [`keymap`]: Linux key codes -> [`KeyCode`] + US-layout text.
//! * [`devices`]: sysfs discovery and classification (keyboard / mouse),
//!   skipping the onboard `zpod-buttons` device (apps already read it).
//! * [`hid`]: a reader thread with hotplug (rescan every second) feeding a
//!   channel; or, with `ZPOD_HID_SIM=<script>`, a scripted simulator.
//! * [`role`]: USB controller role (gadget / host) for honest UI states.
//!
//! Typed text is never logged: [`InputEvent`]'s `Debug` redacts it.

pub mod devices;
pub mod event;
pub mod hid;
pub mod keymap;
pub mod normalize;
pub mod role;

pub use event::{Control, DeviceInfo, HidEvent, InputEvent, Phase, PointerButton, SourceId, SourceKind};
pub use hid::HidInput;
pub use keymap::KeyCode;
pub use role::UsbRole;

/// Monotonic clock in microseconds (same clock as the evdev timestamps after
/// `EVIOCSCLOCKID(CLOCK_MONOTONIC)`).
pub fn now_us() -> u64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    ts.tv_sec as u64 * 1_000_000 + ts.tv_nsec as u64 / 1000
}
