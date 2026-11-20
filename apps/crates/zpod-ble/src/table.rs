//! Per-address rows for BLE Lab, plus the JSONL capture line format.

use std::fmt::Write;

use crate::ad::{self, Ad};
use crate::classify::{classify, Class};
use crate::hci::AdvReport;

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub addr: String,
    pub addr_kind: &'static str,
    pub name: Option<String>,
    pub class: Class,
    pub rssi: i8,
    pub rssi_min: i8,
    pub rssi_max: i8,
    pub tx_power: Option<i8>,
    pub count: u32,
    pub first_ms: u64,
    pub last_ms: u64,
    pub connectable: bool,
    /// last advertising data and last scan response (raw)
    pub adv: Vec<u8>,
    pub scan_rsp: Vec<u8>,
    pub ad: Ad,
}

impl Row {
    pub fn title(&self) -> String {
        match (&self.name, &self.class) {
            (_, Class::RemoteId(_, m)) => m
                .iter()
                .find_map(|x| if let zpod_remoteid::Message::BasicId(b) = x { Some(format!("Drone {}", b.uas_id)) } else { None })
                .unwrap_or_else(|| "Drone (Remote ID)".into()),
            (Some(n), _) if !n.is_empty() => n.clone(),
            (_, c) => c.short(),
        }
    }

    /// Detail lines (screen + text capture).
    pub fn lines(&self, now_ms: u64) -> Vec<String> {
        let mut v = vec![
            format!("{}  ({})", self.addr, self.addr_kind),
            format!("Class: {}", self.class.short()),
            format!("RSSI {} dBm  (min {} max {})  {} adv", self.rssi, self.rssi_min, self.rssi_max, self.count),
            format!("Seen {:.1} s ago, for {:.0} s{}", now_ms.saturating_sub(self.last_ms) as f32 / 1000.0, self.last_ms.saturating_sub(self.first_ms) as f32 / 1000.0, if self.connectable { ", connectable" } else { "" }),
        ];
        if let Some(n) = &self.name {
            v.push(format!("Name: {n}"));
        }
        if let Some(t) = self.tx_power.or(self.ad.tx_power) {
            v.push(format!("TX power {t} dBm"));
        }
        if let Some(f) = self.ad.flags {
            v.push(format!("Flags 0x{f:02x}{}{}", if f & 0x02 != 0 { " general-disc" } else { "" }, if f & 0x04 != 0 { " LE-only" } else { "" }));
        }
        for u in &self.ad.uuid16 {
            v.push(format!("Service 0x{u:04X}"));
        }
        for (c, d) in &self.ad.mfr {
            v.push(format!("Mfr 0x{c:04X} {}: {}", crate::classify::company(*c).unwrap_or("?"), hex(d)));
        }
        for (u, d) in &self.ad.service_data {
            v.push(format!("SvcData 0x{u:04X}: {}", hex(d)));
        }
        if let Class::RemoteId(c, msgs) = &self.class {
            v.push(format!("Remote ID counter {c}"));
            for m in msgs {
                v.extend(rid_lines(m));
            }
        }
        if let Class::IBeacon { uuid, major, minor, power } = &self.class {
            v.push(format!("UUID {uuid}"));
            v.push(format!("major {major} minor {minor} @1m {power} dBm"));
        }
        v.push(format!("ADV {}", hex(&self.adv)));
        if !self.scan_rsp.is_empty() {
            v.push(format!("RSP {}", hex(&self.scan_rsp)));
        }
        v
    }
}

pub use zpod_remoteid::display::lines as rid_lines;

pub fn hex(b: &[u8]) -> String {
    let mut s = String::with_capacity(b.len() * 2);
    for x in b {
        let _ = write!(s, "{x:02x}");
    }
    s
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Sort {
    #[default]
    Signal,
    Recent,
    Kind,
}

impl Sort {
    pub fn next(self) -> Sort {
        match self {
            Sort::Signal => Sort::Recent,
            Sort::Recent => Sort::Kind,
            Sort::Kind => Sort::Signal,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Sort::Signal => "signal",
            Sort::Recent => "recent",
            Sort::Kind => "kind",
        }
    }
}

#[derive(Default)]
pub struct Table {
    pub rows: Vec<Row>,
    pub reports: u64,
    pub max_rows: usize,
}

impl Table {
    pub fn new(max_rows: usize) -> Table {
        Table { rows: Vec::new(), reports: 0, max_rows }
    }

    /// Fold one report in; returns the row index.
    pub fn feed(&mut self, r: &AdvReport, now_ms: u64) -> Option<usize> {
        self.reports += 1;
        let i = match self.rows.iter().position(|x| x.addr == r.addr) {
            Some(i) => i,
            None => {
                if self.max_rows > 0 && self.rows.len() >= self.max_rows {
                    // evict the stalest row
                    let (j, _) = self.rows.iter().enumerate().min_by_key(|(_, x)| x.last_ms)?;
                    self.rows.swap_remove(j);
                }
                self.rows.push(Row {
                    addr: r.addr.clone(),
                    addr_kind: r.addr_kind(),
                    name: None,
                    class: Class::Generic,
                    rssi: r.rssi,
                    rssi_min: r.rssi,
                    rssi_max: r.rssi,
                    tx_power: None,
                    count: 0,
                    first_ms: now_ms,
                    last_ms: now_ms,
                    connectable: false,
                    adv: Vec::new(),
                    scan_rsp: Vec::new(),
                    ad: Ad::default(),
                });
                self.rows.len() - 1
            }
        };
        let row = &mut self.rows[i];
        row.count = row.count.saturating_add(1);
        row.last_ms = now_ms;
        if r.rssi != 127 {
            row.rssi = r.rssi;
            row.rssi_min = row.rssi_min.min(r.rssi);
            row.rssi_max = row.rssi_max.max(r.rssi);
        }
        if r.tx_power.is_some() {
            row.tx_power = r.tx_power;
        }
        if r.scan_response() {
            row.scan_rsp = r.data.clone();
        } else {
            row.adv = r.data.clone();
            row.connectable = r.connectable();
        }
        // classify on adv + scan response together
        let mut all = row.adv.clone();
        all.extend_from_slice(&row.scan_rsp);
        let a = ad::parse(&all);
        if a.name.is_some() {
            row.name = a.name.clone();
        }
        let c = classify(&a);
        // keep a known class if this packet alone says less (e.g. scan rsp)
        if c != Class::Generic || row.class == Class::Generic {
            row.class = c;
        }
        row.ad = a;
        Some(i)
    }

    pub fn expire(&mut self, now_ms: u64, max_age_ms: u64) {
        self.rows.retain(|r| now_ms.saturating_sub(r.last_ms) <= max_age_ms);
    }

    pub fn sorted(&self, s: Sort) -> Vec<usize> {
        let mut v: Vec<usize> = (0..self.rows.len()).collect();
        match s {
            Sort::Signal => v.sort_by_key(|&i| (std::cmp::Reverse(self.rows[i].class.notable()), std::cmp::Reverse(self.rows[i].rssi))),
            Sort::Recent => v.sort_by_key(|&i| std::cmp::Reverse(self.rows[i].last_ms)),
            Sort::Kind => v.sort_by_key(|&i| (self.rows[i].class.short(), std::cmp::Reverse(self.rows[i].rssi))),
        }
        v
    }
}

/// One JSONL capture line for a report.
pub fn jsonl(r: &AdvReport, t_ms: u64, class: &str) -> String {
    format!(
        "{{\"t_ms\":{},\"addr\":\"{}\",\"addr_type\":{},\"rssi\":{},\"evt\":{},\"ext\":{},\"class\":\"{}\",\"data\":\"{}\"}}\n",
        t_ms,
        r.addr,
        r.addr_type,
        r.rssi,
        r.event_type,
        r.extended,
        class.replace(['"', '\\'], ""),
        hex(&r.data)
    )
}
