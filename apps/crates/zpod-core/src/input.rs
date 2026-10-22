//! Logical ZPOD buttons and events.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    /// D-pad centre (all four directions together) = OK.
    Ok,
    Home,
    PlayPause,
    Triangle,
    Square,
    Circle,
    /// X = cancel.
    Cross,
    VolUp,
    VolDown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyState {
    Press,
    /// Kernel autorepeat while held.
    Repeat,
    Release,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Event {
    pub key: Key,
    pub state: KeyState,
}

impl Event {
    pub const fn press(key: Key) -> Self {
        Self { key, state: KeyState::Press }
    }
    /// Press or autorepeat (what menus react to).
    pub fn is_down(&self) -> bool {
        matches!(self.state, KeyState::Press | KeyState::Repeat)
    }
}

impl Key {
    /// Linux input key code -> ZPOD key, as published by zpod-buttons
    /// (overlay/usr/lib/zpod/zpod_buttons.py). ESC/BACKSPACE map to Cross so
    /// a desktop keyboard works too.
    pub fn from_linux_code(code: u16) -> Option<Key> {
        Some(match code {
            103 => Key::Up,          // KEY_UP      GPIO4
            108 => Key::Down,        // KEY_DOWN    GPIO22
            105 => Key::Left,        // KEY_LEFT    GPIO17
            106 => Key::Right,       // KEY_RIGHT   GPIO23
            28 | 96 => Key::Ok,      // KEY_ENTER (4-way combo), KEY_KPENTER
            102 => Key::Home,        // KEY_HOME    GPIO24
            164 | 57 => Key::PlayPause, // KEY_PLAYPAUSE GPIO6 (KEY_SPACE on desktops)
            23 => Key::Triangle,     // KEY_I       GPIO13
            22 => Key::Square,       // KEY_U       GPIO12
            37 => Key::Circle,       // KEY_K       GPIO26
            36 | 1 | 14 => Key::Cross, // KEY_J GPIO16, KEY_ESC, KEY_BACKSPACE
            115 => Key::VolUp,       // KEY_VOLUMEUP   GPIO5
            114 => Key::VolDown,     // KEY_VOLUMEDOWN GPIO20
            _ => return None,
        })
    }

    pub fn from_name(s: &str) -> Option<Key> {
        Some(match s {
            "up" => Key::Up,
            "down" => Key::Down,
            "left" => Key::Left,
            "right" => Key::Right,
            "ok" | "center" | "enter" => Key::Ok,
            "home" => Key::Home,
            "play" | "playpause" => Key::PlayPause,
            "triangle" => Key::Triangle,
            "square" => Key::Square,
            "circle" => Key::Circle,
            "x" | "cross" => Key::Cross,
            "volup" => Key::VolUp,
            "voldown" => Key::VolDown,
            _ => return None,
        })
    }
}
