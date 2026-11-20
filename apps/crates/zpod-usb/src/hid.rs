//! HID report descriptor (HID 1.11, 6.2.2) summary: top-level
//! application collections and the size of each report.

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Collection {
    pub usage_page: u16,
    pub usage: u16,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    pub id: u8,
    pub input_bits: u32,
    pub output_bits: u32,
    pub feature_bits: u32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct HidSummary {
    pub collections: Vec<Collection>,
    pub reports: Vec<Report>,
    pub items: u32,
    pub warnings: Vec<String>,
}

impl HidSummary {
    pub fn is_keyboard(&self) -> bool {
        self.collections.iter().any(|c| c.usage_page == 1 && c.usage == 6)
    }
    pub fn is_mouse(&self) -> bool {
        self.collections.iter().any(|c| c.usage_page == 1 && c.usage == 2)
    }
    pub fn labels(&self) -> Vec<String> {
        self.collections.iter().map(|c| usage_label(c.usage_page, c.usage)).collect()
    }
}

pub fn usage_label(page: u16, usage: u16) -> String {
    match (page, usage) {
        (1, 1) => "Pointer".into(),
        (1, 2) => "Mouse".into(),
        (1, 4) => "Joystick".into(),
        (1, 5) => "Gamepad".into(),
        (1, 6) => "Keyboard".into(),
        (1, 7) => "Keypad".into(),
        (1, 0x80) => "System Control".into(),
        (0x0C, 1) => "Consumer Control".into(),
        (0x0D, 2) => "Pen".into(),
        (0x0D, 4) => "Touch Screen".into(),
        (0x0D, 5) => "Touch Pad".into(),
        (0x0D, _) => "Digitizer".into(),
        (0x0B, _) => "Telephony".into(),
        (0x84, _) | (0x85, _) => "Power / UPS".into(),
        (0x20, _) => "Sensor".into(),
        (0xF1D0, 1) => "FIDO U2F".into(),
        (p, u) if p >= 0xFF00 => format!("Vendor {p:04x}:{u:04x}"),
        (p, u) => format!("Page {p:04x} usage {u:04x}"),
    }
}

pub fn parse(b: &[u8]) -> HidSummary {
    let mut s = HidSummary::default();
    let (mut page, mut size, mut count, mut id) = (0u16, 0u32, 0u32, 0u8);
    let mut usage: Option<(u16, u16)> = None;
    let mut depth = 0u32;
    let mut stack: Vec<(u16, u32, u32, u8)> = Vec::new();
    let mut i = 0usize;
    let report = |s: &mut HidSummary, id: u8| -> usize {
        match s.reports.iter().position(|r| r.id == id) {
            Some(i) => i,
            None => {
                s.reports.push(Report { id, ..Default::default() });
                s.reports.len() - 1
            }
        }
    };
    while i < b.len() {
        let pre = b[i];
        if pre == 0xFE {
            // long item: bDataSize, bLongItemTag, data
            let Some(&n) = b.get(i + 1) else { break };
            i += 3 + n as usize;
            continue;
        }
        let n = match pre & 3 {
            3 => 4,
            x => x as usize,
        };
        if i + 1 + n > b.len() {
            s.warnings.push(format!("item at {i} runs past the end"));
            break;
        }
        let mut v: u32 = 0;
        for k in 0..n {
            v |= (b[i + 1 + k] as u32) << (8 * k);
        }
        let (ty, tag) = ((pre >> 2) & 3, pre >> 4);
        s.items += 1;
        match (ty, tag) {
            // main
            (0, 0x8) | (0, 0x9) | (0, 0xB) => {
                let r = report(&mut s, id);
                let bits = size.saturating_mul(count);
                match tag {
                    0x8 => s.reports[r].input_bits += bits,
                    0x9 => s.reports[r].output_bits += bits,
                    _ => s.reports[r].feature_bits += bits,
                }
                usage = None;
            }
            (0, 0xA) => {
                if depth == 0 && (v & 0xff) == 1 {
                    let (up, u) = usage.unwrap_or((page, 0));
                    s.collections.push(Collection { usage_page: up, usage: u });
                }
                depth += 1;
                usage = None;
            }
            (0, 0xC) => depth = depth.saturating_sub(1),
            // global
            (1, 0x0) => page = v as u16,
            (1, 0x7) => size = v,
            (1, 0x8) => id = v as u8,
            (1, 0x9) => count = v,
            (1, 0xA) => stack.push((page, size, count, id)),
            (1, 0xB) => {
                if let Some((p, sz, c, d)) = stack.pop() {
                    (page, size, count, id) = (p, sz, c, d);
                }
            }
            // local usage (a 4-byte usage carries its own page)
            (2, 0x0) => {
                if usage.is_none() {
                    let up = if n == 4 { (v >> 16) as u16 } else { page };
                    usage = Some((up, v as u16));
                }
            }
            _ => {}
        }
        i += 1 + n;
    }
    s
}
