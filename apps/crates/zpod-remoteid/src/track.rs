// Copyright (C) 2026 Paul Lambert
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! Aircraft table: merge messages from one transmitter (MAC) into one row.

use crate::msg::{BasicId, Location, Message, OperatorId, SelfId, System};
use crate::transport::Transport;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Aircraft {
    /// transmitter address (BLE or Wi-Fi MAC), "aa:bb:..."
    pub mac: String,
    pub transport: Option<Transport>,
    pub basic: Vec<BasicId>,
    pub location: Option<Location>,
    pub system: Option<System>,
    pub self_id: Option<SelfId>,
    pub operator: Option<OperatorId>,
    pub rssi: Option<i8>,
    pub channel: Option<u32>,
    pub last_counter: Option<u8>,
    pub frames: u32,
    pub first_ms: u64,
    pub last_ms: u64,
    pub auth_pages: u32,
}

impl Aircraft {
    /// Best label: first serial / registration id, else the MAC.
    pub fn label(&self) -> String {
        self.basic.iter().find(|b| !b.uas_id.is_empty()).map(|b| b.uas_id.clone()).unwrap_or_else(|| self.mac.clone())
    }
    pub fn emergency(&self) -> bool {
        self.location.as_ref().is_some_and(|l| l.status == 3)
    }
}

#[derive(Default)]
pub struct Tracker {
    pub aircraft: Vec<Aircraft>,
    /// drop rows not heard for this long (ms); 0 = keep
    pub expire_ms: u64,
}

impl Tracker {
    pub fn new(expire_ms: u64) -> Tracker {
        Tracker { aircraft: Vec::new(), expire_ms }
    }

    pub fn feed(&mut self, mac: &str, t: Transport, counter: u8, msgs: &[Message], rssi: Option<i8>, channel: Option<u32>, now_ms: u64) -> usize {
        let i = match self.aircraft.iter().position(|a| a.mac == mac) {
            Some(i) => i,
            None => {
                self.aircraft.push(Aircraft { mac: mac.to_string(), first_ms: now_ms, ..Default::default() });
                self.aircraft.len() - 1
            }
        };
        let a = &mut self.aircraft[i];
        a.transport = Some(t);
        a.frames = a.frames.saturating_add(1);
        a.last_ms = now_ms;
        a.last_counter = Some(counter);
        if rssi.is_some() {
            a.rssi = rssi;
        }
        if channel.is_some() {
            a.channel = channel;
        }
        for m in msgs {
            match m {
                Message::BasicId(b) => {
                    if let Some(x) = a.basic.iter_mut().find(|x| x.id_type == b.id_type) {
                        *x = b.clone();
                    } else if a.basic.len() < 2 {
                        a.basic.push(b.clone());
                    }
                }
                Message::Location(l) => a.location = Some(l.clone()),
                Message::System(s) => a.system = Some(s.clone()),
                Message::SelfId(s) => a.self_id = Some(s.clone()),
                Message::OperatorId(o) => a.operator = Some(o.clone()),
                Message::Auth { .. } => a.auth_pages += 1,
                Message::Unknown(_) => {}
            }
        }
        i
    }

    pub fn expire(&mut self, now_ms: u64) {
        if self.expire_ms > 0 {
            let e = self.expire_ms;
            self.aircraft.retain(|a| now_ms.saturating_sub(a.last_ms) <= e);
        }
    }
}
