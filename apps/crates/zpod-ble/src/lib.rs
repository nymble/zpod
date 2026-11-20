//! zpod-ble: what Bluetooth LE devices around the ZPOD are advertising.
//!
//! Receive only. Advertising reports arrive as HCI events on the kernel's
//! HCI *monitor* channel (read-only by design; the root helper `zpod-labd`
//! opens it and streams the packets, and asks BlueZ to keep LE discovery
//! running). Nothing here can advertise, connect, pair or send.
//!
//! * [`hci`]: monitor header + LE (extended) advertising report parsing.
//! * [`ad`]: advertising data structures.
//! * [`classify`]: what kind of thing sent it.
//! * [`table`]: per-address rows for the list and detail screens; JSONL.

pub mod ad;
pub mod classify;
pub mod hci;
pub mod table;

pub use classify::{classify, Class};
pub use hci::{parse_monitor, AdvReport};
pub use table::{Row, Table};

#[cfg(test)]
mod tests;
