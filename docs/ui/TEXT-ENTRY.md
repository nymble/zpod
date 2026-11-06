# Text and password entry (`zpod_ui::textentry`)

A reusable entry screen for 320x240 in the shared UI crate (`no_std` +
`alloc`, embedded-graphics), first used for **joining a Wi-Fi network with
a password** (Wi-Fi > Networks nearby > network > OK = Join).

## Two ways to type

- **USB keyboard** (optional): when the USB port is in host mode and a
  keyboard is plugged in, type directly. Enter = submit, Esc = cancel,
  Backspace, Ctrl+U = clear, Tab = show/hide. `zpod-input` rescans every
  second, so plugging in or pulling the keyboard switches views live.
- **Buttons**, always: a 10-column character grid.
  - Arrows move (wrap around, repeat while held); **OK/Circle** types the
    highlighted character or runs the highlighted action.
  - **Square** = next character set: `abc` (a-z), `ABC` (A-Z), `123` (digits
    and `!@#$%^&*()-_=+.,:;?/`), `#+=` (`'"`~\|<>[]{}`). With the Space cell
    that is every printable ASCII character 0x20-0x7E, each reachable once.
  - **Triangle** = show/hide; **X** = delete the last character, or cancel
    when the field is empty.
  - Bottom row (fixed): set tabs, Space, Delete, [kbd], Done.
  - `kbd` appears only when no keyboard is present, the port is a gadget
    and the role can be switched: it opens the existing USB host confirmation
    (switching the role is never done remotely).
- With a keyboard present the grid is hidden; any arrow brings it back,
  typing hides it again.

Masked by default (dots). Max 63 characters, ASCII only (WPA passphrase
rules; a raw 64-hex-digit key cannot be typed here). The field wipes its memory when dropped; `Debug` prints only the
length; no `Outcome` carries the text. The launcher drops the entry as soon
as the page leaves the stack.

## Wi-Fi join (unchanged backend)

`zpod_hal::wifijoin`: the passphrase goes to `/usr/lib/zpod/zpod-wpa-psk`
on stdin only (never argv or the environment), which derives the WPA key;
only the derived key is stored, in `/data/etc/wpa_supplicant.conf` (root,
0600).
A failed join restores the previous network, and "Joined" is shown only
once the interface has an IP address. Nothing logs the password or SSID.

## API

```rust
use zpod_ui::textentry::{TextEntry, Pad, KeyIn, Outcome};
let mut e = TextEntry::wifi_password();
e.set_keyboard(has_keyboard);       // on every poll (hotplug)
e.pick.set_offer_keyboard(can_switch_to_host);
match e.pad(Pad::Select) { Outcome::Submit => ..., Outcome::Cancel => ..., Outcome::WantKeyboard => ..., _ => {} }
e.key(KeyIn::Char('x'));
e.draw(&mut display, "Join network", "Password", "Join", kb_help, note);
```

Tests: `cargo test -p zpod-ui` (character coverage, grid fits 320x240,
navigation, typing every symbol by buttons, edit/delete/reveal/done,
X-then-cancel, length/ASCII limits, keyboard hotplug fallback, secrets kept
out of `Debug`). Simulator screenshots: `apps/tools/sim/password/run.sh`.
