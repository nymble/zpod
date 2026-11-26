// Copyright (C) 2026 Paul Lambert
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
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
//! | `wlan-stream <iface>` | radiotap frames from a monitor interface (receive) |
//! | `hci-stream` | HCI monitor-channel packets + passive LE scan on hci0 |
//! | `tx-arm <secs>` | arm over-the-air / HID transmit for 1..=120 s (off at boot) |
//! | `wlan-tx-rid <iface> <chan> <secs>` | inject moving TEST Remote ID beacons (needs arm) |
//! | `hid-arm-ok` | consume the arm for a HID / DuckyScript run (app writes /dev/hidg0) |
//!
//! Transmit paths require a fresh `tx-arm` every time. The UI must show the
//! safeguard warning ([`proto::TX_SAFEGUARD`]) and get an explicit confirm
//! before calling `tx-arm`. Nothing is armed at boot.

pub mod client;
pub mod proto;

pub const SOCKET: &str = "/run/zpod-labd.sock";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
