//! Launcher menu tree and navigation (pure logic, unit-tested).
//!
//! Conventions (docs/ui/BUTTONS-AND-MENUS.md): Up/Down move, Right/OK/Circle
//! enter, Left/X back one level, Home = top menu, Play/Pause = music,
//! Vol+/- = volume (overlay drawn by the platform layer). Volume is NOT a
//! menu item, and no button is dedicated to an app (Clock is a menu entry).

use zpod_core::Nav;
use zpod_ui::widgets::ListState;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuId {
    Top,
    Settings,
    Display,
    Diagnostics,
    Power,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    /// Installed app, index into the launcher's app list.
    App(usize),
    Transfer,
    Wifi,
    Menu(MenuId),
    BacklightTest,
    SpiSpeed,
    DateTime,
    About,
    Apps,
    Status,
    ButtonTest,
    Tone,
    Reboot,
    Shutdown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Menu(MenuId),
    Transfer,
    Wifi,
    DateTime,
    About,
    Apps,
    Status,
    ButtonTest,
    /// true = reboot, false = shut down
    Confirm(bool),
}

/// What the launcher must do after a key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    None,
    Redraw,
    Launch(usize),
    PlayPause,
    Tone,
    BacklightTest,
    CycleSpi,
    RefreshWifi,
    /// Entered a page that loads data (Wi-Fi info, apps list, date editor).
    Opened(Page),
    Power(bool),
}

pub fn items(m: MenuId, n_apps: usize) -> Vec<Item> {
    match m {
        MenuId::Top => {
            let mut v: Vec<Item> = (0..n_apps).map(Item::App).collect();
            v.extend([Item::Transfer, Item::Wifi, Item::Menu(MenuId::Settings), Item::Menu(MenuId::Power)]);
            v
        }
        MenuId::Settings => vec![
            Item::Menu(MenuId::Display),
            Item::DateTime,
            Item::About,
            Item::Apps,
            Item::Menu(MenuId::Diagnostics),
        ],
        MenuId::Display => vec![Item::BacklightTest, Item::SpiSpeed],
        MenuId::Diagnostics => vec![Item::Status, Item::ButtonTest, Item::Tone],
        MenuId::Power => vec![Item::Reboot, Item::Shutdown],
    }
}

/// (title, subtitle) for built-in items; apps are labelled by the launcher.
pub fn label(it: Item) -> (&'static str, &'static str) {
    match it {
        Item::App(_) => ("", ""),
        Item::Transfer => ("File transfer", "USB cable or Wi-Fi"),
        Item::Wifi => ("Wi-Fi info", "SSID, IP, MAC, signal"),
        Item::Menu(MenuId::Settings) => ("Settings", "Display, time, battery, diagnostics"),
        Item::Menu(MenuId::Power) => ("Power", "Reboot, shut down"),
        Item::Menu(MenuId::Display) => ("Display", "Backlight, SPI speed"),
        Item::Menu(MenuId::Diagnostics) => ("Diagnostics", "Status, button test, tone test"),
        Item::Menu(MenuId::Top) => ("ZPOD", ""),
        Item::BacklightTest => ("Backlight test", "GPIO27 off for 3 s (inferred pin)"),
        Item::SpiSpeed => ("Panel SPI speed", "32 MHz safe; 48/64 MHz test"),
        Item::DateTime => ("Date & time", "Set clock, saves to RTC"),
        Item::About => ("Battery & about", ""),
        Item::Apps => ("Installed apps", ""),
        Item::Status => ("System status", ""),
        Item::ButtonTest => ("Button test", ""),
        Item::Tone => ("Tone test (440 Hz)", ""),
        Item::Reboot => ("Reboot", ""),
        Item::Shutdown => ("Shut down", ""),
    }
}

pub fn title(p: Page) -> &'static str {
    match p {
        Page::Menu(MenuId::Top) => "ZPOD",
        Page::Menu(MenuId::Settings) => "Settings",
        Page::Menu(MenuId::Display) => "Display",
        Page::Menu(MenuId::Diagnostics) => "Diagnostics",
        Page::Menu(MenuId::Power) => "Power",
        Page::Transfer => "File transfer",
        Page::Wifi => "Wi-Fi",
        Page::DateTime => "Date & time",
        Page::About => "Battery & about",
        Page::Apps => "Installed apps",
        Page::Status => "System status",
        Page::ButtonTest => "Button test",
        Page::Confirm(_) => "Power",
    }
}

pub struct Menus {
    pub stack: Vec<(Page, ListState)>,
}

impl Default for Menus {
    fn default() -> Self {
        Self::new()
    }
}

impl Menus {
    pub fn new() -> Self {
        Menus { stack: vec![(Page::Menu(MenuId::Top), ListState::default())] }
    }

    pub fn page(&self) -> Page {
        self.stack.last().unwrap().0
    }

    pub fn list(&self) -> &ListState {
        &self.stack.last().unwrap().1
    }

    pub fn list_mut(&mut self) -> &mut ListState {
        &mut self.stack.last_mut().unwrap().1
    }

    #[allow(dead_code)] // tests + future breadcrumb
    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    /// Home: straight back to the top launcher menu (selection kept).
    pub fn home(&mut self) {
        self.stack.truncate(1);
    }

    /// Back one level (no-op at the top).
    pub fn back(&mut self) {
        if self.stack.len() > 1 {
            self.stack.pop();
        }
    }

    pub fn push(&mut self, p: Page) {
        self.stack.push((p, ListState::default()));
    }

    /// The item under the cursor, if the current page is a menu.
    #[allow(dead_code)]
    pub fn selected(&self, n_apps: usize) -> Option<Item> {
        match self.page() {
            Page::Menu(m) => items(m, n_apps).get(self.list().sel).copied(),
            _ => None,
        }
    }

    /// Conventional handling for menus and simple pages. Pages with their
    /// own bindings (DateTime editor, ButtonTest) are handled by the caller.
    /// `rows` = (number of rows on this page, rows visible).
    pub fn handle(&mut self, nav: Nav, n_apps: usize, rows: (usize, usize)) -> Effect {
        match nav {
            Nav::Home => {
                self.home();
                return Effect::Redraw;
            }
            Nav::PlayPause => return Effect::PlayPause,
            Nav::Volume | Nav::Other(_) => return Effect::None,
            _ => {}
        }
        let page = self.page();
        if let Page::Confirm(reboot) = page {
            return match nav {
                Nav::Enter => Effect::Power(reboot),
                Nav::Back => {
                    self.back();
                    Effect::Redraw
                }
                _ => Effect::None,
            };
        }
        match nav {
            Nav::Back => {
                self.back();
                Effect::Redraw
            }
            Nav::Up | Nav::Down => {
                let (len, vis) = match page {
                    Page::Menu(m) => (items(m, n_apps).len(), rows.1),
                    _ => rows,
                };
                if len == 0 {
                    return Effect::None;
                }
                self.list_mut().step(if nav == Nav::Up { -1 } else { 1 }, len, vis.max(1));
                Effect::Redraw
            }
            Nav::Enter => match page {
                Page::Menu(m) => {
                    let Some(it) = items(m, n_apps).get(self.list().sel).copied() else { return Effect::None };
                    self.enter(it)
                }
                Page::Wifi => Effect::RefreshWifi,
                _ => Effect::None,
            },
            _ => Effect::None,
        }
    }

    fn enter(&mut self, it: Item) -> Effect {
        let open = |s: &mut Self, p: Page| {
            s.push(p);
            Effect::Opened(p)
        };
        match it {
            Item::App(i) => Effect::Launch(i),
            Item::Menu(m) => open(self, Page::Menu(m)),
            Item::Transfer => open(self, Page::Transfer),
            Item::Wifi => open(self, Page::Wifi),
            Item::DateTime => open(self, Page::DateTime),
            Item::About => open(self, Page::About),
            Item::Apps => open(self, Page::Apps),
            Item::Status => open(self, Page::Status),
            Item::ButtonTest => open(self, Page::ButtonTest),
            Item::Reboot => open(self, Page::Confirm(true)),
            Item::Shutdown => open(self, Page::Confirm(false)),
            Item::Tone => Effect::Tone,
            Item::BacklightTest => Effect::BacklightTest,
            Item::SpiSpeed => Effect::CycleSpi,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROWS: (usize, usize) = (0, 4);

    fn select(m: &mut Menus, n_apps: usize, it: Item) {
        let Page::Menu(id) = m.page() else { panic!("not a menu") };
        let i = items(id, n_apps).iter().position(|x| *x == it).expect("item in menu");
        m.list_mut().sel = i;
    }

    #[test]
    fn no_volume_item_and_tone_only_in_diagnostics() {
        for id in [MenuId::Top, MenuId::Settings, MenuId::Display, MenuId::Diagnostics, MenuId::Power] {
            for it in items(id, 3) {
                assert!(!label(it).0.to_lowercase().contains("volume"), "{it:?}");
            }
        }
        assert!(!items(MenuId::Top, 3).contains(&Item::Tone));
        assert!(!items(MenuId::Settings, 3).contains(&Item::Tone));
        assert!(items(MenuId::Diagnostics, 3).contains(&Item::Tone));
    }

    #[test]
    fn left_is_back_one_level_everywhere_and_right_enters() {
        let mut m = Menus::new();
        select(&mut m, 2, Item::Menu(MenuId::Settings));
        assert_eq!(m.handle(Nav::Enter, 2, ROWS), Effect::Opened(Page::Menu(MenuId::Settings)));
        select(&mut m, 2, Item::Menu(MenuId::Diagnostics));
        m.handle(Nav::Enter, 2, ROWS);
        assert_eq!(m.page(), Page::Menu(MenuId::Diagnostics));
        assert_eq!(m.depth(), 3);
        m.handle(Nav::Back, 2, ROWS);
        assert_eq!(m.page(), Page::Menu(MenuId::Settings));
        m.handle(Nav::Back, 2, ROWS);
        assert_eq!(m.page(), Page::Menu(MenuId::Top));
        m.handle(Nav::Back, 2, ROWS); // no-op at the top
        assert_eq!(m.depth(), 1);
        select(&mut m, 2, Item::Wifi);
        m.handle(Nav::Enter, 2, ROWS);
        assert_eq!(m.page(), Page::Wifi);
        assert_eq!(m.handle(Nav::Enter, 2, ROWS), Effect::RefreshWifi);
        m.handle(Nav::Back, 2, ROWS);
        assert_eq!(m.page(), Page::Menu(MenuId::Top));
    }

    #[test]
    fn home_always_returns_to_top() {
        let mut m = Menus::new();
        select(&mut m, 0, Item::Menu(MenuId::Settings));
        m.handle(Nav::Enter, 0, ROWS);
        select(&mut m, 0, Item::Menu(MenuId::Display));
        m.handle(Nav::Enter, 0, ROWS);
        assert_eq!(m.depth(), 3);
        assert_eq!(m.handle(Nav::Home, 0, ROWS), Effect::Redraw);
        assert_eq!(m.page(), Page::Menu(MenuId::Top));
        assert_eq!(m.depth(), 1);
    }

    #[test]
    fn play_pause_is_global_never_tone() {
        let mut m = Menus::new();
        assert_eq!(m.handle(Nav::PlayPause, 1, ROWS), Effect::PlayPause);
        select(&mut m, 1, Item::Menu(MenuId::Settings));
        m.handle(Nav::Enter, 1, ROWS);
        select(&mut m, 1, Item::Menu(MenuId::Diagnostics));
        m.handle(Nav::Enter, 1, ROWS);
        assert_eq!(m.handle(Nav::PlayPause, 1, ROWS), Effect::PlayPause);
        select(&mut m, 1, Item::Tone);
        assert_eq!(m.handle(Nav::Enter, 1, ROWS), Effect::Tone);
    }

    #[test]
    fn apps_launch_and_confirm_power() {
        let mut m = Menus::new();
        assert_eq!(m.handle(Nav::Enter, 3, ROWS), Effect::Launch(0));
        m.handle(Nav::Down, 3, ROWS);
        assert_eq!(m.handle(Nav::Enter, 3, ROWS), Effect::Launch(1));
        select(&mut m, 3, Item::Menu(MenuId::Power));
        m.handle(Nav::Enter, 3, ROWS);
        select(&mut m, 3, Item::Shutdown);
        m.handle(Nav::Enter, 3, ROWS);
        assert_eq!(m.page(), Page::Confirm(false));
        assert_eq!(m.handle(Nav::Up, 3, ROWS), Effect::None);
        assert_eq!(m.handle(Nav::Back, 3, ROWS), Effect::Redraw);
        assert_eq!(m.page(), Page::Menu(MenuId::Power));
        select(&mut m, 3, Item::Reboot);
        m.handle(Nav::Enter, 3, ROWS);
        assert_eq!(m.handle(Nav::Enter, 3, ROWS), Effect::Power(true));
    }

    #[test]
    fn up_down_wrap_in_menus() {
        let mut m = Menus::new();
        m.handle(Nav::Up, 0, ROWS);
        assert_eq!(m.selected(0), Some(Item::Menu(MenuId::Power)));
        m.handle(Nav::Down, 0, ROWS);
        assert_eq!(m.selected(0), Some(Item::Transfer));
    }

    #[test]
    fn volume_and_app_local_keys_do_nothing_in_menus() {
        let mut m = Menus::new();
        assert_eq!(m.handle(Nav::Volume, 0, ROWS), Effect::None);
        assert_eq!(m.handle(Nav::Other(zpod_core::Key::Triangle), 0, ROWS), Effect::None);
        assert_eq!(m.handle(Nav::Other(zpod_core::Key::Square), 0, ROWS), Effect::None);
        assert_eq!(m.depth(), 1);
    }
}
