//! zpod-wlan: listen to 802.11 with a second, monitor-capable USB adapter.
//!
//! Policy (docs/ROADMAP-LABS.md): receive only; the onboard Broadcom radio
//! (brcmfmac, SDIO) is never put in monitor mode, it keeps the user's
//! network. Mode changes and the raw capture socket belong to the root
//! helper `zpod-labd`; this crate is the pure part: inventory, parsers,
//! pcap files and the Remote ID aircraft table. `testgen` writes OFFLINE
//! test pcaps of a labelled, moving TEST aircraft (no radio involved).

pub mod frame;
pub mod inventory;
pub mod pcap;
pub mod radiotap;
pub mod rid;
pub mod testgen;

pub use inventory::{inventory, Adapter};
pub use rid::{decode_frame, Decoded};

#[cfg(test)]
mod tests;
