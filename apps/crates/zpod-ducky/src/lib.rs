// Copyright (C) 2026 Paul Lambert
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! DuckyScript (USB Rubber Ducky / Bash Bunny QUACK subset) for authorised
//! lab testing on the operator's own machines.
//!
//! Supported: `REM`, `DELAY`, `STRING`, `STRINGLN`, `ENTER`, `TAB`, `SPACE`,
//! `ESCAPE`/`ESC`, `BACKSPACE`/`DELETE`, arrows, `HOME`/`END`/`PAGEUP`/
//! `PAGEDOWN`, `F1`..`F12`, modifier chords (`GUI`/`WINDOWS`/`COMMAND`,
//! `CTRL`/`CONTROL`, `ALT`, `SHIFT`) with an optional key or character.
//! Unknown commands are hard errors (never silently skipped).
//!
//! HID encoding uses the 8-byte boot keyboard report (modifiers + 6 keycodes).


/// One parsed action.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    DelayMs(u32),
    /// Type a string (US QWERTY); each char is press+release.
    String(String),
    /// One key (usage) with modifiers bitmask.
    Key { modifiers: u8, key: u8 },
}

/// Modifier bits (USB HID boot keyboard).
pub const MOD_CTRL: u8 = 0x01;
pub const MOD_SHIFT: u8 = 0x02;
pub const MOD_ALT: u8 = 0x04;
pub const MOD_GUI: u8 = 0x08;

/// Parse a whole script. `REM` lines and blank lines are ignored.
pub fn parse(script: &str) -> Result<Vec<Action>, String> {
    let mut out = Vec::new();
    for (n, raw) in script.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("REM") || line.starts_with("rem") {
            continue;
        }
        out.push(parse_line(line).map_err(|e| format!("line {}: {e}", n + 1))?);
    }
    Ok(out)
}

fn parse_line(line: &str) -> Result<Action, String> {
    let (cmd, rest) = match line.split_once(char::is_whitespace) {
        Some((c, r)) => (c, r.trim()),
        None => (line, ""),
    };
    let u = cmd.to_ascii_uppercase();
    match u.as_str() {
        "DELAY" => {
            let ms: u32 = rest.parse().map_err(|_| format!("DELAY needs ms, got {rest:?}"))?;
            if ms > 60_000 {
                return Err("DELAY max 60000 ms".into());
            }
            Ok(Action::DelayMs(ms))
        }
        "STRING" => Ok(Action::String(rest.into())),
        "STRINGLN" => {
            let mut s = String::from(rest);
            // encoded as string then Enter
            Ok(Action::String({
                // We expand STRINGLN into string + Enter via a marker newline
                // consumed by encode: trailing \n means Enter after the text.
                s.push('\n');
                s
            }))
        }
        "ENTER" => Ok(Action::Key { modifiers: 0, key: 0x28 }),
        "TAB" => Ok(Action::Key { modifiers: 0, key: 0x2b }),
        "SPACE" => Ok(Action::Key { modifiers: 0, key: 0x2c }),
        "ESCAPE" | "ESC" => Ok(Action::Key { modifiers: 0, key: 0x29 }),
        "BACKSPACE" | "DELETE" => Ok(Action::Key { modifiers: 0, key: 0x2a }),
        "UP" | "UPARROW" => Ok(Action::Key { modifiers: 0, key: 0x52 }),
        "DOWN" | "DOWNARROW" => Ok(Action::Key { modifiers: 0, key: 0x51 }),
        "LEFT" | "LEFTARROW" => Ok(Action::Key { modifiers: 0, key: 0x50 }),
        "RIGHT" | "RIGHTARROW" => Ok(Action::Key { modifiers: 0, key: 0x4f }),
        "HOME" => Ok(Action::Key { modifiers: 0, key: 0x4a }),
        "END" => Ok(Action::Key { modifiers: 0, key: 0x4d }),
        "PAGEUP" => Ok(Action::Key { modifiers: 0, key: 0x4b }),
        "PAGEDOWN" => Ok(Action::Key { modifiers: 0, key: 0x4e }),
        "GUI" | "WINDOWS" | "COMMAND" | "CTRL" | "CONTROL" | "ALT" | "SHIFT" => chord(&u, rest),
        x if x.starts_with('F') && x.len() <= 3 => {
            let n: u8 = x[1..].parse().map_err(|_| format!("bad key {x}"))?;
            if !(1..=12).contains(&n) {
                return Err(format!("bad function key {x}"));
            }
            Ok(Action::Key { modifiers: 0, key: 0x3a + n - 1 })
        }
        _ => Err(format!("unknown command {cmd:?}")),
    }
}

fn chord(mod_name: &str, rest: &str) -> Result<Action, String> {
    let mut modifiers = match mod_name {
        "GUI" | "WINDOWS" | "COMMAND" => MOD_GUI,
        "CTRL" | "CONTROL" => MOD_CTRL,
        "ALT" => MOD_ALT,
        "SHIFT" => MOD_SHIFT,
        _ => 0,
    };
    if rest.is_empty() {
        return Ok(Action::Key { modifiers, key: 0 });
    }
    let parts: Vec<&str> = rest.split_whitespace().collect();
    let key_tok = parts[parts.len() - 1];
    for p in &parts[..parts.len() - 1] {
        match p.to_ascii_uppercase().as_str() {
            "GUI" | "WINDOWS" | "COMMAND" => modifiers |= MOD_GUI,
            "CTRL" | "CONTROL" => modifiers |= MOD_CTRL,
            "ALT" => modifiers |= MOD_ALT,
            "SHIFT" => modifiers |= MOD_SHIFT,
            other => return Err(format!("bad modifier {other}")),
        }
    }
    let key = named_or_char(key_tok, &mut modifiers)?;
    Ok(Action::Key { modifiers, key })
}

fn named_or_char(tok: &str, modifiers: &mut u8) -> Result<u8, String> {
    let u = tok.to_ascii_uppercase();
    Ok(match u.as_str() {
        "ENTER" => 0x28,
        "TAB" => 0x2b,
        "SPACE" => 0x2c,
        "ESCAPE" | "ESC" => 0x29,
        "BACKSPACE" | "DELETE" => 0x2a,
        "UP" | "UPARROW" => 0x52,
        "DOWN" | "DOWNARROW" => 0x51,
        "LEFT" | "LEFTARROW" => 0x50,
        "RIGHT" | "RIGHTARROW" => 0x4f,
        "HOME" => 0x4a,
        "END" => 0x4d,
        "PAGEUP" => 0x4b,
        "PAGEDOWN" => 0x4e,
        _ if tok.chars().count() == 1 => {
            let ch = tok.chars().next().unwrap();
            let (k, sh) = char_key(ch).ok_or_else(|| format!("no key for {tok:?}"))?;
            if sh {
                *modifiers |= MOD_SHIFT;
            }
            k
        }
        _ => return Err(format!("bad key {tok}")),
    })
}

/// US QWERTY: returns (hid_keycode, needs_shift).
pub fn char_key(c: char) -> Option<(u8, bool)> {
    match c {
        'a'..='z' => Some((0x04 + (c as u8 - b'a'), false)),
        'A'..='Z' => Some((0x04 + (c as u8 - b'A'), true)),
        '1'..='9' => Some((0x1e + (c as u8 - b'1'), false)),
        '0' => Some((0x27, false)),
        '\n' | '\r' => Some((0x28, false)),
        ' ' => Some((0x2c, false)),
        '-' => Some((0x2d, false)),
        '=' => Some((0x2e, false)),
        '[' => Some((0x2f, false)),
        ']' => Some((0x30, false)),
        '\\' => Some((0x31, false)),
        ';' => Some((0x33, false)),
        '\'' => Some((0x34, false)),
        '`' => Some((0x35, false)),
        ',' => Some((0x36, false)),
        '.' => Some((0x37, false)),
        '/' => Some((0x38, false)),
        '!' => Some((0x1e, true)),
        '@' => Some((0x1f, true)),
        '#' => Some((0x20, true)),
        '$' => Some((0x21, true)),
        '%' => Some((0x22, true)),
        '^' => Some((0x23, true)),
        '&' => Some((0x24, true)),
        '*' => Some((0x25, true)),
        '(' => Some((0x26, true)),
        ')' => Some((0x27, true)),
        '_' => Some((0x2d, true)),
        '+' => Some((0x2e, true)),
        '{' => Some((0x2f, true)),
        '}' => Some((0x30, true)),
        '|' => Some((0x31, true)),
        ':' => Some((0x33, true)),
        '"' => Some((0x34, true)),
        '~' => Some((0x35, true)),
        '<' => Some((0x36, true)),
        '>' => Some((0x37, true)),
        '?' => Some((0x38, true)),
        '\t' => Some((0x2b, false)),
        _ => None,
    }
}

/// 8-byte boot keyboard report.
pub fn report(modifiers: u8, key: u8) -> [u8; 8] {
    let mut r = [0u8; 8];
    r[0] = modifiers;
    r[2] = key;
    r
}

pub fn report_release() -> [u8; 8] {
    [0u8; 8]
}

/// Expand actions into a dry-run description (one line per step).
pub fn dry_run(actions: &[Action]) -> Vec<String> {
    let mut v = Vec::new();
    for a in actions {
        match a {
            Action::DelayMs(ms) => v.push(format!("wait {ms} ms")),
            Action::String(s) => {
                let (text, enter) = if let Some(t) = s.strip_suffix('\n') {
                    (t, true)
                } else {
                    (s.as_str(), false)
                };
                v.push(format!("type {text:?}"));
                if enter {
                    v.push("key Enter".into());
                }
            }
            Action::Key { modifiers, key } => v.push(format!("key mod={modifiers:#04x} code={key:#04x}")),
        }
    }
    v
}

/// Encode actions to a sequence of (delay_ms_before, report). Releases are
/// inserted after every key. The caller writes each report to `/dev/hidg0`
/// and sleeps `delay_ms_before`.
pub fn encode(actions: &[Action]) -> Result<Vec<(u32, [u8; 8])>, String> {
    let mut out = Vec::new();
    for a in actions {
        match a {
            Action::DelayMs(ms) => out.push((*ms, report_release())),
            Action::String(s) => {
                for ch in s.chars() {
                    if ch == '\n' {
                        out.push((0, report(0, 0x28)));
                        out.push((20, report_release()));
                        continue;
                    }
                    let (k, sh) = char_key(ch).ok_or_else(|| format!("cannot type {ch:?} on US layout"))?;
                    let m = if sh { MOD_SHIFT } else { 0 };
                    out.push((0, report(m, k)));
                    out.push((20, report_release()));
                }
            }
            Action::Key { modifiers, key } => {
                out.push((0, report(*modifiers, *key)));
                out.push((30, report_release()));
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic() {
        let a = parse("REM hi\nDELAY 100\nSTRING hello\nENTER\nGUI r\n").unwrap();
        assert_eq!(a[0], Action::DelayMs(100));
        assert_eq!(a[1], Action::String("hello".into()));
        assert_eq!(a[2], Action::Key { modifiers: 0, key: 0x28 });
        assert!(matches!(a[3], Action::Key { modifiers: MOD_GUI, key: 0x15 }));
    }

    #[test]
    fn stringln_and_encode() {
        let a = parse("STRINGLN ZPOD LAB TEST\n").unwrap();
        let d = dry_run(&a);
        assert_eq!(d[0], "type \"ZPOD LAB TEST\"");
        assert_eq!(d[1], "key Enter");
        let e = encode(&a).unwrap();
        assert!(e.len() > 10);
        assert_eq!(e.last().unwrap().1, [0u8; 8]);
    }

    #[test]
    fn rejects_unknown() {
        assert!(parse("ATTACKMODE HID\n").is_err());
        assert!(parse("DELAY 999999\n").is_err());
    }

    #[test]
    fn char_map_smoke() {
        assert_eq!(char_key('a'), Some((0x04, false)));
        assert_eq!(char_key('A'), Some((0x04, true)));
        assert_eq!(char_key('!'), Some((0x1e, true)));
    }
}
