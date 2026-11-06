//! Linux key codes -> [`KeyCode`] and US-layout text.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KeyCode {
    Esc,
    Enter,
    Backspace,
    Tab,
    Space,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Delete,
    Insert,
    F(u8),
    Shift,
    Ctrl,
    Alt,
    Meta,
    CapsLock,
    /// A key that produces text (letters, digits, punctuation, keypad
    /// digits): its Linux code. Use [`InputEvent::text`](crate::InputEvent)
    /// for the character.
    Char(u16),
    /// Placeholder used only when printing events (redaction).
    Printable,
    Other(u16),
}

impl KeyCode {
    pub fn is_printable(self) -> bool {
        matches!(self, KeyCode::Char(_) | KeyCode::Space)
    }
}

pub const KEY_ESC: u16 = 1;
pub const KEY_BACKSPACE: u16 = 14;
pub const KEY_TAB: u16 = 15;
pub const KEY_ENTER: u16 = 28;
pub const KEY_LEFTCTRL: u16 = 29;
pub const KEY_LEFTSHIFT: u16 = 42;
pub const KEY_RIGHTSHIFT: u16 = 54;
pub const KEY_LEFTALT: u16 = 56;
pub const KEY_SPACE: u16 = 57;
pub const KEY_CAPSLOCK: u16 = 58;
pub const KEY_KPENTER: u16 = 96;
pub const KEY_RIGHTCTRL: u16 = 97;
pub const KEY_RIGHTALT: u16 = 100;
pub const KEY_HOME: u16 = 102;
pub const KEY_UP: u16 = 103;
pub const KEY_PAGEUP: u16 = 104;
pub const KEY_LEFT: u16 = 105;
pub const KEY_RIGHT: u16 = 106;
pub const KEY_END: u16 = 107;
pub const KEY_DOWN: u16 = 108;
pub const KEY_PAGEDOWN: u16 = 109;
pub const KEY_INSERT: u16 = 110;
pub const KEY_DELETE: u16 = 111;
pub const KEY_LEFTMETA: u16 = 125;
pub const KEY_RIGHTMETA: u16 = 126;

/// (code, plain, shifted) for the main block, US layout.
const CHARS: &[(u16, char, char)] = &[
    (2, '1', '!'), (3, '2', '@'), (4, '3', '#'), (5, '4', '$'), (6, '5', '%'),
    (7, '6', '^'), (8, '7', '&'), (9, '8', '*'), (10, '9', '('), (11, '0', ')'),
    (12, '-', '_'), (13, '=', '+'),
    (16, 'q', 'Q'), (17, 'w', 'W'), (18, 'e', 'E'), (19, 'r', 'R'), (20, 't', 'T'),
    (21, 'y', 'Y'), (22, 'u', 'U'), (23, 'i', 'I'), (24, 'o', 'O'), (25, 'p', 'P'),
    (26, '[', '{'), (27, ']', '}'),
    (30, 'a', 'A'), (31, 's', 'S'), (32, 'd', 'D'), (33, 'f', 'F'), (34, 'g', 'G'),
    (35, 'h', 'H'), (36, 'j', 'J'), (37, 'k', 'K'), (38, 'l', 'L'),
    (39, ';', ':'), (40, '\'', '"'), (41, '`', '~'), (43, '\\', '|'),
    (44, 'z', 'Z'), (45, 'x', 'X'), (46, 'c', 'C'), (47, 'v', 'V'), (48, 'b', 'B'),
    (49, 'n', 'N'), (50, 'm', 'M'), (51, ',', '<'), (52, '.', '>'), (53, '/', '?'),
    (86, '\\', '|'),
    // keypad (as if Num Lock is on)
    (55, '*', '*'), (71, '7', '7'), (72, '8', '8'), (73, '9', '9'), (74, '-', '-'),
    (75, '4', '4'), (76, '5', '5'), (77, '6', '6'), (78, '+', '+'), (79, '1', '1'),
    (80, '2', '2'), (81, '3', '3'), (82, '0', '0'), (83, '.', '.'), (98, '/', '/'),
];

pub fn key_code(code: u16) -> KeyCode {
    match code {
        KEY_ESC => KeyCode::Esc,
        KEY_ENTER | KEY_KPENTER => KeyCode::Enter,
        KEY_BACKSPACE => KeyCode::Backspace,
        KEY_TAB => KeyCode::Tab,
        KEY_SPACE => KeyCode::Space,
        KEY_UP => KeyCode::Up,
        KEY_DOWN => KeyCode::Down,
        KEY_LEFT => KeyCode::Left,
        KEY_RIGHT => KeyCode::Right,
        KEY_HOME => KeyCode::Home,
        KEY_END => KeyCode::End,
        KEY_PAGEUP => KeyCode::PageUp,
        KEY_PAGEDOWN => KeyCode::PageDown,
        KEY_INSERT => KeyCode::Insert,
        KEY_DELETE => KeyCode::Delete,
        KEY_LEFTSHIFT | KEY_RIGHTSHIFT => KeyCode::Shift,
        KEY_LEFTCTRL | KEY_RIGHTCTRL => KeyCode::Ctrl,
        KEY_LEFTALT | KEY_RIGHTALT => KeyCode::Alt,
        KEY_LEFTMETA | KEY_RIGHTMETA => KeyCode::Meta,
        KEY_CAPSLOCK => KeyCode::CapsLock,
        59..=68 => KeyCode::F((code - 58) as u8),
        87 => KeyCode::F(11),
        88 => KeyCode::F(12),
        c if CHARS.iter().any(|e| e.0 == c) => KeyCode::Char(c),
        c => KeyCode::Other(c),
    }
}

/// Text for `code` with Shift / Caps Lock (Caps Lock only affects letters).
pub fn key_text(code: u16, shift: bool, caps: bool) -> Option<char> {
    if code == KEY_SPACE {
        return Some(' ');
    }
    let &(_, plain, shifted) = CHARS.iter().find(|e| e.0 == code)?;
    let upper = if plain.is_ascii_lowercase() { shift ^ caps } else { shift };
    Some(if upper { shifted } else { plain })
}

/// Linux code (and whether Shift is needed) that types `c` (simulator, tests).
pub fn code_for_char(c: char) -> Option<(u16, bool)> {
    if c == ' ' {
        return Some((KEY_SPACE, false));
    }
    // main block first (so '1' is the top-row 1, not the keypad)
    CHARS.iter().find_map(|&(code, p, s)| {
        if p == c {
            Some((code, false))
        } else if s == c && s != p {
            Some((code, true))
        } else {
            None
        }
    })
}

/// Names for the simulator / fixtures ("enter", "esc", "up", "a", "f1", ...).
pub fn code_for_name(name: &str) -> Option<u16> {
    let n = name.to_ascii_lowercase();
    Some(match n.as_str() {
        "esc" | "escape" => KEY_ESC,
        "enter" | "return" => KEY_ENTER,
        "backspace" | "bs" => KEY_BACKSPACE,
        "tab" => KEY_TAB,
        "space" => KEY_SPACE,
        "up" => KEY_UP,
        "down" => KEY_DOWN,
        "left" => KEY_LEFT,
        "right" => KEY_RIGHT,
        "home" => KEY_HOME,
        "end" => KEY_END,
        "pageup" => KEY_PAGEUP,
        "pagedown" => KEY_PAGEDOWN,
        "delete" | "del" => KEY_DELETE,
        "shift" | "lshift" => KEY_LEFTSHIFT,
        "rshift" => KEY_RIGHTSHIFT,
        "ctrl" => KEY_LEFTCTRL,
        "alt" => KEY_LEFTALT,
        "capslock" | "caps" => KEY_CAPSLOCK,
        _ => {
            if let Some(f) = n.strip_prefix('f').and_then(|d| d.parse::<u16>().ok()) {
                return match f {
                    1..=10 => Some(58 + f),
                    11 => Some(87),
                    12 => Some(88),
                    _ => None,
                };
            }
            let mut ch = n.chars();
            match (ch.next(), ch.next()) {
                (Some(c), None) => return code_for_char(c).map(|x| x.0),
                _ => return None,
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_digits_shift_caps() {
        assert_eq!(key_code(30), KeyCode::Char(30));
        assert_eq!(key_text(30, false, false), Some('a'));
        assert_eq!(key_text(30, true, false), Some('A'));
        assert_eq!(key_text(30, false, true), Some('A'));
        assert_eq!(key_text(30, true, true), Some('a'));
        assert_eq!(key_text(2, false, true), Some('1'), "caps lock leaves digits");
        assert_eq!(key_text(2, true, false), Some('!'));
        assert_eq!(key_text(KEY_SPACE, false, false), Some(' '));
        assert_eq!(key_text(KEY_ENTER, false, false), None);
        assert_eq!(key_code(KEY_KPENTER), KeyCode::Enter);
        assert_eq!(key_code(60), KeyCode::F(2));
    }

    #[test]
    fn round_trip_every_printable_ascii() {
        for b in 0x20u8..0x7f {
            let c = b as char;
            let (code, shift) = code_for_char(c).unwrap_or_else(|| panic!("no key for {c:?}"));
            assert_eq!(key_text(code, shift, false), Some(c), "{c:?}");
        }
    }

    #[test]
    fn names() {
        assert_eq!(code_for_name("Enter"), Some(KEY_ENTER));
        assert_eq!(code_for_name("f12"), Some(88));
        assert_eq!(code_for_name("q"), Some(16));
        assert_eq!(code_for_name("nope"), None);
    }
}
