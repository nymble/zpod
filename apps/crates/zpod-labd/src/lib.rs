//! zpod-labd protocol, policy and client.
//!
//! One connection = one line command (ASCII, max 128 bytes), answered by
//! one line: `ok ...` or `err <reason>`. The two `*-stream` commands then
//! keep the connection open and send frames `[u32 LE length][bytes]`
//! (length 0 = keep-alive) until the client hangs up; the daemon then
//! undoes what the stream needed (Wi-Fi back to managed, BLE scan off).
//!
//! | command | does |
//! |---|---|
//! | `ping` | `ok zpod-labd <version>` |
//! | `wlan-monitor <iface> <chan>` | USB adapter only: down, type monitor, up, channel |
//! | `wlan-chan <iface> <chan>` | channel on an adapter already in monitor mode |
//! | `wlan-managed <iface>` | back to managed (never touches the onboard radio) |
//! | `wlan-stream <iface>` | radiotap frames from a monitor interface (receive only) |
//! | `hci-stream` | HCI monitor-channel packets + passive LE scan on hci0 |
//!
//! Nothing here transmits: no injection socket is handed out, the HCI
//! monitor channel is read-only, and the BLE scan is passive (no scan
//! requests).

pub mod client;
pub mod proto;

pub const SOCKET: &str = "/run/zpod-labd.sock";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
