//! Golden fixture: raw evdev log -> normalized begin/update/end events
//! (spec §4.7 / acceptance C5). Generators see only these fields.

use zpod_input::normalize::Normalizer;
use zpod_input::{Control, KeyCode, Phase, PointerButton};

fn control_name(c: Control) -> String {
    match c {
        Control::Key(KeyCode::Char(_)) => "key:char".into(),
        Control::Key(k) => format!("key:{}", format!("{k:?}").to_lowercase()),
        Control::Button(PointerButton::Left) => "button:left".into(),
        Control::Button(b) => format!("button:{}", format!("{b:?}").to_lowercase()),
        Control::Pointer => "pointer".into(),
        Control::Wheel => "wheel".into(),
    }
}

#[test]
fn keyboard_mouse_fixture_matches_golden() {
    let log = include_str!("fixtures/usb-keyboard-mouse.evlog");
    let want = include_str!("fixtures/usb-keyboard-mouse.expected");
    let mut norms: Vec<Normalizer> = vec![];
    let mut cur = 0usize;
    let mut out = vec![];
    let mut last_t = 0;
    for line in log.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        if let Some(rest) = line.strip_prefix('@') {
            let id: u32 = rest.split_whitespace().next().unwrap().parse().unwrap();
            norms.push(Normalizer::new(id));
            cur = norms.len() - 1;
            continue;
        }
        let f: Vec<i64> = line.split_whitespace().map(|x| x.parse().unwrap()).collect();
        last_t = f[0] as u64;
        norms[cur].feed(f[0] as u64, f[1] as u16, f[2] as u16, f[3] as i32, &mut out);
    }
    for n in norms.iter_mut() {
        n.tick(last_t + 1_000_000, &mut out);
    }
    let got: Vec<String> = out
        .iter()
        .map(|e| {
            let ph = match e.phase {
                Phase::Begin => "begin",
                Phase::Update => "update",
                Phase::End => "end",
            };
            format!("{} {} {} {} {} {} {} {}", e.t_us, e.source, e.contact, ph, control_name(e.control), e.dx, e.dy, e.text.map(|c| c.to_string()).unwrap_or("-".into()))
        })
        .collect();
    let want: Vec<&str> = want.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()).collect();
    assert_eq!(got, want);
}
