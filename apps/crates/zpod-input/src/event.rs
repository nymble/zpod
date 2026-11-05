//! The normalized event model shared by all ZPOD input adapters.

use crate::keymap::KeyCode;

/// One per device for the lifetime of the reader (not reused after unplug).
pub type SourceId = u32;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SourceKind {
    /// The onboard buttons (`zpod-buttons`), when adapted into this model.
    Buttons,
    Keyboard,
    Mouse,
}

impl SourceKind {
    pub fn label(self) -> &'static str {
        match self {
            SourceKind::Buttons => "buttons",
            SourceKind::Keyboard => "keyboard",
            SourceKind::Mouse => "mouse",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Begin,
    Update,
    End,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerButton {
    Left,
    Right,
    Middle,
    Side,
    Extra,
    Other(u16),
}

/// What an event is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    /// A keyboard key. Contact id = the Linux key code.
    /// Begin = press, Update = autorepeat, End = release.
    Key(KeyCode),
    /// A mouse button. Contact id = 0x100 + button index.
    /// Begin = press, End = release.
    Button(PointerButton),
    /// Relative pointer motion. Contact id 0. Begin = first motion after
    /// [`crate::normalize::POINTER_IDLE_US`] of rest, Update = more motion,
    /// End = rest again (or unplug). `dx`/`dy` in device counts, y down.
    Pointer,
    /// Wheel detents, one Update per report. Contact id 0x200. `dy` > 0 = up
    /// (away from the user), `dx` = horizontal wheel.
    Wheel,
}

pub const CONTACT_POINTER: u32 = 0;
pub const CONTACT_BUTTON_BASE: u32 = 0x100;
pub const CONTACT_WHEEL: u32 = 0x200;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct InputEvent {
    /// Monotonic microseconds (`CLOCK_MONOTONIC`).
    pub t_us: u64,
    pub source: SourceId,
    pub contact: u32,
    pub phase: Phase,
    pub control: Control,
    pub dx: i32,
    pub dy: i32,
    /// Text the key produces with the current modifiers (US layout), on
    /// Begin and Update only. Redacted in `Debug`.
    pub text: Option<char>,
    /// Shift / Ctrl / Alt held when this event was produced.
    pub mods: Mods,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub meta: bool,
}

impl core::fmt::Debug for InputEvent {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // Never print typed characters: they may be a password.
        f.debug_struct("InputEvent")
            .field("t_us", &self.t_us)
            .field("source", &self.source)
            .field("contact", &match self.control {
                Control::Key(k) if k.is_printable() => None,
                _ => Some(self.contact),
            })
            .field("phase", &self.phase)
            .field("control", &match self.control {
                Control::Key(k) if k.is_printable() => Control::Key(KeyCode::Printable),
                c => c,
            })
            .field("dx", &self.dx)
            .field("dy", &self.dy)
            .field("text", &self.text.map(|_| "<redacted>"))
            .finish()
    }
}

impl InputEvent {
    pub fn is_begin(&self) -> bool {
        self.phase == Phase::Begin
    }
    /// Begin or autorepeat Update of a key.
    pub fn key_down(&self) -> Option<KeyCode> {
        match (self.control, self.phase) {
            (Control::Key(k), Phase::Begin | Phase::Update) => Some(k),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceInfo {
    pub id: SourceId,
    pub kind: SourceKind,
    /// Device name from the kernel (e.g. "Logitech USB Receiver").
    pub name: String,
    /// /dev/input/eventN (or "sim").
    pub path: String,
    /// bus type (0x03 USB, 0x05 Bluetooth), vendor, product
    pub bus: u16,
    pub vendor: u16,
    pub product: u16,
}

impl DeviceInfo {
    pub fn bus_label(&self) -> &'static str {
        match self.bus {
            0x03 => "USB",
            0x05 => "Bluetooth",
            0x06 => "virtual",
            _ => "input",
        }
    }
}

/// What the reader thread delivers.
#[derive(Clone, Debug, PartialEq)]
pub enum HidEvent {
    Added(DeviceInfo),
    Removed(DeviceInfo),
    Input(InputEvent),
}
