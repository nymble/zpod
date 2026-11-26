// Copyright (C) 2026 Paul Lambert
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! Display helpers shared by the lab apps.

use crate::track::Aircraft;

/// One Remote ID message as short display lines (BLE Lab, Wi-Fi Lab).
pub fn lines(m: &crate::Message) -> Vec<String> {
    use crate::msg::{id_type_label, status_label, ua_type_label};
    use crate::Message as M;
    let f = |o: Option<f32>, u: &str| o.map(|x| format!("{x:.1}{u}")).unwrap_or_else(|| "?".into());
    match m {
        M::BasicId(b) => vec![format!("ID {} ({}, {})", b.uas_id, id_type_label(b.id_type), ua_type_label(b.ua_type))],
        M::Location(l) => vec![
            format!("{} {}", status_label(l.status), match (l.lat, l.lon) { (Some(a), Some(o)) => format!("{a:.6}, {o:.6}"), _ => "no position".into() }),
            format!("alt geo {} baro {} h {}", f(l.alt_geo, "m"), f(l.alt_baro, "m"), f(l.height, "m")),
            format!("spd {} vs {} trk {}", f(l.speed_h, "m/s"), f(l.speed_v, "m/s"), l.track_deg.map(|d| format!("{d}°")).unwrap_or_else(|| "?".into())),
        ],
        M::System(s) => vec![format!("Operator {}", match (s.op_lat, s.op_lon) { (Some(a), Some(o)) => format!("{a:.6}, {o:.6}"), _ => "no position".into() })],
        M::SelfId(s) => vec![format!("Self-ID: {}", s.text)],
        M::OperatorId(o) => vec![format!("Operator ID: {}", o.id)],
        M::Auth { auth_type, page } => vec![format!("Auth type {auth_type} page {page}")],
        M::Unknown(t) => vec![format!("Message type {t}")],
    }
}


/// Detail lines for one aircraft (merged messages).
pub fn aircraft_lines(a: &Aircraft, now_ms: u64) -> Vec<String> {
    let mut v = vec![format!("# {}", a.label())];
    v.push(format!(
        "{}  {}  {} frames, last {:.1} s ago",
        a.mac,
        a.transport.map(|t| t.label()).unwrap_or("?"),
        a.frames,
        now_ms.saturating_sub(a.last_ms) as f32 / 1000.0
    ));
    let mut sig = String::new();
    if let Some(r) = a.rssi {
        sig.push_str(&format!("RSSI {r} dBm  "));
    }
    if let Some(c) = a.channel {
        sig.push_str(&format!("ch {c}  "));
    }
    if let Some(c) = a.last_counter {
        sig.push_str(&format!("counter {c}"));
    }
    if !sig.is_empty() {
        v.push(sig);
    }
    if a.emergency() {
        v.push("! EMERGENCY status".into());
    }
    for b in &a.basic {
        v.extend(lines(&crate::Message::BasicId(b.clone())));
    }
    if let Some(l) = &a.location {
        v.extend(lines(&crate::Message::Location(l.clone())));
    }
    if let Some(s) = &a.system {
        v.extend(lines(&crate::Message::System(s.clone())));
    }
    if let Some(s) = &a.self_id {
        v.extend(lines(&crate::Message::SelfId(s.clone())));
    }
    if let Some(o) = &a.operator {
        v.extend(lines(&crate::Message::OperatorId(o.clone())));
    }
    if a.auth_pages > 0 {
        v.push(format!("Auth pages received: {}", a.auth_pages));
    }
    v
}
