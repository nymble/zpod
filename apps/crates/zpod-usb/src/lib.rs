// Copyright (C) 2026 Paul Lambert
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! zpod-usb: look at what is plugged into the USB port (host mode) without
//! touching it. Everything comes from files the kernel already exposes
//! (`/sys/bus/usb/devices/*`, each device's raw `descriptors`, and each HID
//! interface's `report_descriptor`), all readable without root, so no
//! device is opened, claimed, reset or sent a control transfer.
//!
//! * [`desc`]: parse the raw device + configuration descriptor blob.
//! * [`hid`]: parse a HID report descriptor into collections and reports.
//! * [`known`]: names for vendors / products / classes, plus user lines
//!   from `/data/zpod/appdata/usb-lab/known.txt`.
//! * [`scan`]: walk sysfs into [`Device`] records; [`dump`]: text / JSON.

pub mod desc;
pub mod dump;
pub mod hid;
pub mod known;
pub mod scan;

pub use scan::{scan, Device, Interface};

#[cfg(test)]
mod tests;
