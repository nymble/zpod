//! Lab app screens on 320x240: a status bar, a header line (pill + text),
//! a 5-row list or a scrolling detail page, and the hint bar. Same look as
//! the launcher's Tools pages.

use embedded_graphics::{pixelcolor::Rgb565, prelude::*, primitives::{PrimitiveStyle, RoundedRectangle}};
use zpod_core::StatusInfo;
use zpod_ui::{fill, rect, status::draw_status_bar, text, text::Align, theme::*, widgets, widgets::{ListState, Row}, Font, Framebuffer};

pub const ROW_H: i32 = 36;
pub const LIST_Y: i32 = STATUS_H + 24;
pub const LIST_H: i32 = 240 - widgets::HINTS_H - LIST_Y;
pub const VISIBLE: usize = (LIST_H / ROW_H) as usize;
pub const DETAIL_LINE_H: i32 = 15;
pub const DETAIL_LINES: usize = ((240 - widgets::HINTS_H - LIST_Y - 4) / DETAIL_LINE_H) as usize;

pub use zpod_ui::theme::{ACCENT, CYAN, DIM, FAINT, FG, OK, WARN};

pub struct Item {
    pub title: String,
    pub sub: String,
    pub right: String,
    pub mark: Option<Rgb565>,
}

/// Status bar + header pill + text.
pub fn header(fb: &mut Framebuffer, st: &StatusInfo, title: &str, pill: &str, pill_bg: Rgb565, info: &str) {
    fill(fb, 0, 0, 320, 240, BG);
    draw_status_bar(fb, title, st);
    let w = text::width(Font::Small, pill) + 12;
    let _ = RoundedRectangle::with_equal_corners(rect(8, STATUS_H + 4, w as u32, 16), Size::new(8, 8)).into_styled(PrimitiveStyle::with_fill(pill_bg)).draw(fb);
    text::draw(fb, Font::Small, pill, 14, STATUS_H + 7, BG);
    text::draw_fit(fb, Font::Small, info, 8 + w + 8, STATUS_H + 7, 312 - (8 + w + 8), DIM);
}

pub fn list(fb: &mut Framebuffer, items: &[Item], ls: &ListState) {
    widgets::draw_list(fb, rect(0, LIST_Y, 320, LIST_H as u32), ROW_H, ls, items.len(), |i| {
        let it = &items[i];
        Row { title: &it.title, sub: Some(&it.sub), right: if it.right.is_empty() { None } else { Some(&it.right) }, icon: None, mark: it.mark }
    });
}

/// Scrolling text page; `scroll` = first line shown. Lines starting with
/// "! " are warnings (red), "# " headings (accent).
pub fn detail(fb: &mut Framebuffer, lines: &[String], scroll: usize) {
    fill(fb, 0, LIST_Y, 320, LIST_H as u32, BG);
    let mut y = LIST_Y + 2;
    for l in lines.iter().skip(scroll).take(DETAIL_LINES) {
        let (s, c) = if let Some(x) = l.strip_prefix("! ") {
            (x, WARN)
        } else if let Some(x) = l.strip_prefix("# ") {
            (x, ACCENT)
        } else {
            (l.as_str(), FG)
        };
        text::draw_fit(fb, Font::Small, s, 8, y, 300, c);
        y += DETAIL_LINE_H;
    }
    if lines.len() > DETAIL_LINES {
        let track = LIST_H - 8;
        let thumb = ((DETAIL_LINES as i32 * track) / lines.len() as i32).max(8);
        let max = lines.len() - DETAIL_LINES;
        let pos = (scroll.min(max) as i32 * (track - thumb)) / max.max(1) as i32;
        fill(fb, 314, LIST_Y + 4, 2, track as u32, FAINT);
        fill(fb, 313, LIST_Y + 4 + pos, 4, thumb as u32, DIM);
    }
}

pub fn max_scroll(lines: usize) -> usize {
    lines.saturating_sub(DETAIL_LINES)
}

/// Centered empty state (no adapter, wrong USB mode...).
pub fn message(fb: &mut Framebuffer, heading: &str, lines: &[&str]) {
    fill(fb, 0, LIST_Y, 320, LIST_H as u32, BG);
    text::draw_aligned(fb, Font::Title, &text::fit(Font::Title, heading, 304), 160, LIST_Y + 34, Align::Center, FG);
    let mut y = LIST_Y + 64;
    for l in lines {
        text::draw_aligned(fb, Font::Small, &text::fit(Font::Small, l, 300), 160, y, Align::Center, DIM);
        y += 17;
    }
}

pub fn hints(fb: &mut Framebuffer, h: &[(&str, &str)]) {
    widgets::hints(fb, h);
}

pub fn toast(fb: &mut Framebuffer, m: &str) {
    widgets::toast(fb, m);
}

/// `/data/zpod/appdata/<app>` (or `$ZPOD_APPDATA/<app>`), created.
pub fn appdata(app: &str) -> std::path::PathBuf {
    let base = std::env::var_os("ZPOD_APPDATA").map(std::path::PathBuf::from).unwrap_or_else(|| "/data/zpod/appdata".into());
    let d = base.join(app);
    let _ = std::fs::create_dir_all(&d);
    d
}

/// UTC "YYYYMMDD-HHMMSS" for file names.
pub fn stamp() -> String {
    let s = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    stamp_of(s)
}

pub fn stamp_of(secs: u64) -> String {
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    // civil-from-days (H. Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}{m:02}{d:02}-{:02}{:02}{:02}", rem / 3600, (rem % 3600) / 60, rem % 60)
}

/// Keep `sel` in range after a list changed size.
pub fn clamp(ls: &mut ListState, len: usize) {
    ls.scroll_into_view(len, VISIBLE);
}

pub fn ago(ms: u64) -> String {
    if ms < 1000 {
        "now".into()
    } else if ms < 60_000 {
        format!("{}s", ms / 1000)
    } else {
        format!("{}m", ms / 60_000)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn stamps() {
        assert_eq!(super::stamp_of(0), "19700101-000000");
        assert_eq!(super::stamp_of(1_795_219_200), "20261121-000000");
        assert_eq!(super::stamp_of(951_786_061), "20000229-010101");
    }
    #[test]
    fn layout_fits() {
        assert_eq!(super::VISIBLE, 5);
        assert!(super::DETAIL_LINES >= 11);
        assert_eq!(super::ago(65_000), "1m");
    }
}

/// "Switch USB?" dialog / "Switching..." / a short note (same as GPS).
pub fn draw_ask(fb: &mut Framebuffer, a: &zpod_input::role::ask::UsbAsk, note: Option<&str>) {
    use zpod_input::role::prompt::Tone;
    let tone = |t: Tone| match t {
        Tone::Normal => FG,
        Tone::Good => OK,
        Tone::Warn => WARN,
        Tone::Dim => DIM,
    };
    if let Some(p) = a.prompt() {
        let lines: Vec<(&str, Rgb565)> = p.lines.iter().map(|(l, t)| (l.as_str(), tone(*t))).collect();
        widgets::confirm(fb, p.title, &lines, if p.ok_switches { ACCENT } else { WARN });
        widgets::hints(fb, &a.hints());
    }
    if let Some(b) = a.busy() {
        widgets::toast(fb, &b);
    } else if let Some(n) = note {
        widgets::toast(fb, n);
    }
}

/// Map a key on the USB dialog to an answer (GPS app conventions).
pub fn ask_answer(k: zpod_core::Key) -> Option<zpod_input::role::ask::Answer> {
    use zpod_core::Key;
    use zpod_input::role::ask::Answer;
    match k {
        Key::Ok | Key::Circle => Some(Answer::Yes),
        Key::Triangle => Some(Answer::Force),
        Key::Cross | Key::Left => Some(Answer::Cancel),
        _ => None,
    }
}
