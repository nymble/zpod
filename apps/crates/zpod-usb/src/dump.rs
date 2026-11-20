//! Human-readable and JSON reports for one device (saved by USB Lab to
//! /data/zpod/appdata/usb-lab/).

use std::fmt::Write;

use crate::hid::usage_label;
use crate::known::class_label;
use crate::scan::Device;

pub fn hex(b: &[u8]) -> String {
    let mut s = String::with_capacity(b.len() * 2);
    for x in b {
        let _ = write!(s, "{x:02x}");
    }
    s
}

pub fn json_str(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            c if (c as u32) < 0x20 => {
                let _ = write!(o, "\\u{:04x}", c as u32);
            }
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

fn opt(s: &Option<String>) -> String {
    s.as_deref().map(json_str).unwrap_or_else(|| "null".into())
}

/// Detail lines for the screen and the text dump.
pub fn lines(d: &Device) -> Vec<String> {
    let mut v = Vec::new();
    v.push(format!("{}  {}", d.id(), d.title()));
    if let Some(k) = &d.known_vendor {
        v.push(format!("Vendor: {k}"));
    }
    if !d.note.is_empty() {
        v.push(format!("Use: {}", d.note));
    }
    v.push(format!("Strings: {} / {}", d.manufacturer.as_deref().unwrap_or("-"), d.product.as_deref().unwrap_or("-")));
    if let Some(s) = &d.serial {
        v.push(format!("Serial: {s}"));
    }
    v.push(format!(
        "Bus {} dev {}  port {}  {} Mb/s  USB {}",
        d.busnum,
        d.devnum,
        d.path,
        d.speed.as_deref().unwrap_or("?"),
        d.version.as_deref().unwrap_or("?")
    ));
    if let Some(dd) = &d.parsed.device {
        v.push(format!("Class {:02x} {}  bcdDevice {:x}.{:02x}  EP0 {}B", dd.class, class_label(dd.class), dd.bcd_device >> 8, dd.bcd_device & 0xff, dd.max_packet0));
    }
    for c in &d.parsed.configs {
        v.push(format!(
            "Config {}: {} mA{}{}  {} iface",
            c.value,
            c.max_power_ma,
            if c.self_powered() { ", self-powered" } else { "" },
            if c.remote_wakeup() { ", wakeup" } else { "" },
            c.interfaces.iter().filter(|i| i.alt == 0).count()
        ));
        for i in &c.interfaces {
            let sys = d.interfaces.iter().find(|s| s.number == i.number);
            let drv = sys.and_then(|s| s.driver.clone()).unwrap_or_else(|| "no driver".into());
            let alt = if i.alt > 0 { format!(" alt {}", i.alt) } else { String::new() };
            v.push(format!(" If {}{}: {} [{}]", i.number, alt, crate::known::iface_label(i.class, i.subclass, i.protocol), drv));
            for e in &i.endpoints {
                v.push(format!("   {}", e.describe()));
            }
            if let Some(n) = i.hid_report_len {
                v.push(format!("   HID report descriptor {n} bytes"));
            }
        }
    }
    for i in &d.interfaces {
        if let Some(h) = &i.hid {
            let cols: Vec<String> = h.collections.iter().map(|c| usage_label(c.usage_page, c.usage)).collect();
            v.push(format!("HID {}: {}", i.number, cols.join(", ")));
            for r in &h.reports {
                v.push(format!("   report {}: in {} out {} feat {} bits", r.id, r.input_bits, r.output_bits, r.feature_bits));
            }
        }
    }
    let ind = d.indicators();
    if ind.is_empty() {
        v.push("Indicators: none".into());
    } else {
        for i in ind {
            v.push(format!("! {i}"));
        }
    }
    for w in &d.parsed.warnings {
        v.push(format!("Descriptor warning: {w}"));
    }
    v
}

pub fn text(devs: &[Device], when: &str) -> String {
    let mut s = format!("ZPOD USB Lab report {when}\n{} device(s)\n", devs.len());
    for d in devs {
        s.push('\n');
        for l in lines(d) {
            s.push_str(&l);
            s.push('\n');
        }
        if !d.raw.is_empty() {
            let _ = writeln!(s, "descriptors: {}", hex(&d.raw));
        }
        for i in &d.interfaces {
            if !i.hid_raw.is_empty() {
                let _ = writeln!(s, "report_descriptor {}: {}", i.number, hex(&i.hid_raw));
            }
        }
    }
    s
}

pub fn json(devs: &[Device], when: &str) -> String {
    let mut s = format!("{{\"format\":\"zpod-usb-report\",\"version\":1,\"time\":{},\"devices\":[", json_str(when));
    for (k, d) in devs.iter().enumerate() {
        if k > 0 {
            s.push(',');
        }
        let _ = write!(
            s,
            "{{\"path\":{},\"bus\":{},\"dev\":{},\"id\":{},\"class\":{},\"manufacturer\":{},\"product\":{},\"serial\":{},\"speed\":{},\"known\":{},\"vendor\":{},\"indicators\":[{}],\"interfaces\":[",
            json_str(&d.path),
            d.busnum,
            d.devnum,
            json_str(&d.id()),
            d.class,
            opt(&d.manufacturer),
            opt(&d.product),
            opt(&d.serial),
            opt(&d.speed),
            opt(&d.known_product),
            opt(&d.known_vendor),
            d.indicators().iter().map(|i| json_str(i)).collect::<Vec<_>>().join(",")
        );
        for (j, i) in d.interfaces.iter().enumerate() {
            if j > 0 {
                s.push(',');
            }
            let hid = match &i.hid {
                Some(h) => format!("{{\"collections\":[{}],\"report_descriptor\":{}}}", h.labels().iter().map(|l| json_str(l)).collect::<Vec<_>>().join(","), json_str(&hex(&i.hid_raw))),
                None => "null".into(),
            };
            let _ = write!(s, "{{\"number\":{},\"class\":{},\"subclass\":{},\"protocol\":{},\"label\":{},\"driver\":{},\"hid\":{}}}", i.number, i.class, i.subclass, i.protocol, json_str(&i.label), opt(&i.driver), hid);
        }
        let _ = write!(s, "],\"descriptors\":{}}}", json_str(&hex(&d.raw)));
    }
    s.push_str("]}\n");
    s
}
