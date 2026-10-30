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
    Tools,
    Power,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    /// Installed app, index into the launcher's app list.
    App(usize),
    Transfer,
    Battery,
    Wifi,
    WifiScan,
    WifiSignal,
    WifiChannels,
    WifiSaved,
    Hotspot,
    DropBox,
    Opds,
    BtScan,
    Menu(MenuId),
    BacklightTest,
    SpiSpeed,
    DateTime,
    About,
    Apps,
    Status,
    Storage,
    ButtonTest,
    Tone,
    Reboot,
    Shutdown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Menu(MenuId),
    Transfer,
    /// M6: charge, time left, history chart
    Battery,
    Wifi,
    WifiScan,
    /// M6: live signal meter (connected network, or the one picked in the scanner)
    WifiSignal,
    /// M6: APs per 2.4 GHz channel
    WifiChannels,
    /// M6: saved networks, OK = switch
    WifiSaved,
    /// M6: access point mode
    Hotspot,
    /// M6: web drop box / OPDS catalog on/off (true = OPDS)
    Server(bool),
    /// M6: first-boot files applied after the first boot (shown at start)
    Notice,
    BtScan,
    /// Details of the selected scan row (Wi-Fi network or BT device).
    ScanDetail,
    DateTime,
    About,
    Apps,
    Status,
    Storage,
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
    /// Wi-Fi tools: OK on a page that does something (switch network,
    /// hotspot on/off, server on/off, open the meter from scan details).
    Activate(Page),
    /// File transfer: OK = rescan the music library now.
    RescanMusic,
    /// Entered a page that loads data (Wi-Fi info, apps list, date editor).
    Opened(Page),
    Power(bool),
}

pub fn items(m: MenuId, n_apps: usize) -> Vec<Item> {
    match m {
        MenuId::Top => {
            let mut v: Vec<Item> = (0..n_apps).map(Item::App).collect();
            v.extend([Item::Transfer, Item::Battery, Item::Menu(MenuId::Tools), Item::Menu(MenuId::Settings), Item::Menu(MenuId::Power)]);
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
        MenuId::Diagnostics => vec![Item::Status, Item::Storage, Item::ButtonTest, Item::Tone],
        MenuId::Tools => vec![
            Item::WifiScan,
            Item::WifiSignal,
            Item::WifiChannels,
            Item::WifiSaved,
            Item::Hotspot,
            Item::DropBox,
            Item::Opds,
            Item::BtScan,
            Item::Wifi,
        ],
        MenuId::Power => vec![Item::Reboot, Item::Shutdown],
    }
}

/// (title, subtitle) for built-in items; apps are labelled by the launcher.
pub fn label(it: Item) -> (&'static str, &'static str) {
    match it {
        Item::App(_) => ("", ""),
        Item::Transfer => ("File transfer", "How to connect, free space, rescan"),
        Item::Battery => ("Battery", "Charge, time left, last charge, history"),
        Item::Wifi => ("Wi-Fi info", "This ZPOD: SSID, IP, MAC, signal"),
        Item::WifiScan => ("Wi-Fi scanner", "Networks nearby: channel, signal, security"),
        Item::WifiSignal => ("Signal meter", "Live signal of the connected network"),
        Item::WifiChannels => ("Channels", "Networks per 2.4 GHz channel, least busy"),
        Item::WifiSaved => ("Saved networks", "Switch to another saved Wi-Fi"),
        Item::Hotspot => ("Hotspot", "ZPOD as a Wi-Fi access point"),
        Item::DropBox => ("Drop box", "Web page: browse, download, upload"),
        Item::Opds => ("Book catalog", "OPDS feed of /data/books for e-readers"),
        Item::BtScan => ("Bluetooth scanner", "Classic + BLE devices nearby, RSSI"),
        Item::Menu(MenuId::Tools) => ("Tools", "Wi-Fi tools, hotspot, drop box, books"),
        Item::Menu(MenuId::Settings) => ("Settings", "Display, time, about, diagnostics"),
        Item::Menu(MenuId::Power) => ("Power", "Reboot, shut down"),
        Item::Menu(MenuId::Display) => ("Display", "Backlight, SPI speed"),
        Item::Menu(MenuId::Diagnostics) => ("Diagnostics", "Status, storage & build, buttons, tone"),
        Item::Menu(MenuId::Top) => ("ZPOD", ""),
        Item::BacklightTest => ("Backlight test", "GPIO27 off for 3 s (inferred pin)"),
        Item::SpiSpeed => ("Panel SPI speed", "32 MHz safe; 48/64 MHz test"),
        Item::DateTime => ("Date & time", "Set clock, saves to RTC"),
        Item::About => ("About", "Version, SD card size and use, RTC"),
        Item::Apps => ("Installed apps", ""),
        Item::Status => ("System status", ""),
        Item::Storage => ("Storage & build", "SD card per partition, image version"),
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
        Page::Menu(MenuId::Tools) => "Tools",
        Page::WifiScan => "Wi-Fi scanner",
        Page::Battery => "Battery",
        Page::WifiSignal => "Signal meter",
        Page::WifiChannels => "Channels",
        Page::WifiSaved => "Saved Wi-Fi",
        Page::Hotspot => "Hotspot",
        Page::Server(false) => "Drop box",
        Page::Server(true) => "Books",
        Page::Notice => "Setup files",
        Page::BtScan => "Bluetooth scanner",
        Page::ScanDetail => "Details",
        Page::Transfer => "File transfer",
        Page::Wifi => "Wi-Fi",
        Page::DateTime => "Date & time",
        Page::About => "About",
        Page::Apps => "Installed apps",
        Page::Status => "System status",
        Page::Storage => "Storage",
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
                Page::Transfer => Effect::RescanMusic,
                Page::WifiSaved if self.list().sel < rows.0 => Effect::Activate(Page::WifiSaved),
                Page::Hotspot | Page::Server(_) => Effect::Activate(page),
                Page::Notice => {
                    self.back();
                    Effect::Activate(Page::Notice)
                }
                Page::ScanDetail if self.stack.len() >= 2 && self.stack[self.stack.len() - 2].0 == Page::WifiScan => {
                    let sel = self.list().sel;
                    self.push(Page::WifiSignal);
                    self.list_mut().sel = sel; // the scan row to follow
                    Effect::Opened(Page::WifiSignal)
                }
                Page::WifiScan | Page::BtScan if self.list().sel < rows.0 => {
                    let sel = self.list().sel;
                    self.push(Page::ScanDetail);
                    self.list_mut().sel = sel; // the row the details are for
                    Effect::Opened(Page::ScanDetail)
                }
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
            Item::Battery => open(self, Page::Battery),
            Item::Wifi => open(self, Page::Wifi),
            Item::WifiScan => open(self, Page::WifiScan),
            Item::WifiSignal => {
                self.push(Page::WifiSignal);
                self.list_mut().sel = usize::MAX; // = the connected network
                Effect::Opened(Page::WifiSignal)
            }
            Item::WifiChannels => open(self, Page::WifiChannels),
            Item::WifiSaved => open(self, Page::WifiSaved),
            Item::Hotspot => open(self, Page::Hotspot),
            Item::DropBox => open(self, Page::Server(false)),
            Item::Opds => open(self, Page::Server(true)),
            Item::BtScan => open(self, Page::BtScan),
            Item::DateTime => open(self, Page::DateTime),
            Item::About => open(self, Page::About),
            Item::Apps => open(self, Page::Apps),
            Item::Status => open(self, Page::Status),
            Item::Storage => open(self, Page::Storage),
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
        for id in [MenuId::Top, MenuId::Settings, MenuId::Display, MenuId::Diagnostics, MenuId::Tools, MenuId::Power] {
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
        select(&mut m, 2, Item::Menu(MenuId::Tools));
        m.handle(Nav::Enter, 2, ROWS);
        select(&mut m, 2, Item::Wifi);
        m.handle(Nav::Enter, 2, ROWS);
        assert_eq!(m.page(), Page::Wifi);
        assert_eq!(m.handle(Nav::Enter, 2, ROWS), Effect::RefreshWifi);
        m.handle(Nav::Back, 2, ROWS);
        assert_eq!(m.page(), Page::Menu(MenuId::Tools));
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
    fn storage_and_build_in_diagnostics() {
        let mut m = Menus::new();
        select(&mut m, 0, Item::Menu(MenuId::Settings));
        m.handle(Nav::Enter, 0, ROWS);
        select(&mut m, 0, Item::Menu(MenuId::Diagnostics));
        m.handle(Nav::Enter, 0, ROWS);
        select(&mut m, 0, Item::Storage);
        assert_eq!(m.handle(Nav::Enter, 0, ROWS), Effect::Opened(Page::Storage));
        assert_eq!(title(m.page()), "Storage");
        m.handle(Nav::Back, 0, ROWS);
        assert_eq!(m.page(), Page::Menu(MenuId::Diagnostics));
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

    #[test]
    fn tools_menu_has_scanners_and_details() {
        assert_eq!(items(MenuId::Top, 1), vec![Item::App(0), Item::Transfer, Item::Battery, Item::Menu(MenuId::Tools), Item::Menu(MenuId::Settings), Item::Menu(MenuId::Power)]);
        assert!(!items(MenuId::Top, 1).contains(&Item::Wifi), "Wi-Fi info moved to Tools");
        let mut m = Menus::new();
        select(&mut m, 0, Item::Menu(MenuId::Tools));
        assert_eq!(m.handle(Nav::Enter, 0, ROWS), Effect::Opened(Page::Menu(MenuId::Tools)));
        assert_eq!(items(MenuId::Tools, 0)[0], Item::WifiScan);
        assert!(items(MenuId::Tools, 0).ends_with(&[Item::BtScan, Item::Wifi]));
        assert_eq!(m.handle(Nav::Enter, 0, ROWS), Effect::Opened(Page::WifiScan));
        // no rows yet: OK does nothing
        assert_eq!(m.handle(Nav::Enter, 0, (0, 5)), Effect::None);
        m.handle(Nav::Down, 0, (3, 5));
        assert_eq!(m.handle(Nav::Enter, 0, (3, 5)), Effect::Opened(Page::ScanDetail));
        assert_eq!(m.list().sel, 1);
        m.handle(Nav::Back, 0, ROWS);
        assert_eq!(m.page(), Page::WifiScan);
        assert_eq!(m.list().sel, 1);
        m.handle(Nav::Back, 0, ROWS);
        select(&mut m, 0, Item::BtScan);
        assert_eq!(m.handle(Nav::Enter, 0, ROWS), Effect::Opened(Page::BtScan));
        assert_eq!(m.handle(Nav::Home, 0, ROWS), Effect::Redraw);
        assert_eq!(m.page(), Page::Menu(MenuId::Top));
    }

    #[test]
    fn m6_battery_and_about_are_separate() {
        assert!(items(MenuId::Top, 0).contains(&Item::Battery));
        assert!(items(MenuId::Settings, 0).contains(&Item::About));
        assert!(!items(MenuId::Settings, 0).contains(&Item::Battery));
        assert_eq!(label(Item::About).0, "About");
        let mut m = Menus::new();
        select(&mut m, 0, Item::Battery);
        assert_eq!(m.handle(Nav::Enter, 0, ROWS), Effect::Opened(Page::Battery));
        assert_eq!(title(m.page()), "Battery");
        m.handle(Nav::Back, 0, ROWS);
        assert_eq!(m.page(), Page::Menu(MenuId::Top));
    }

    #[test]
    fn m6_wifi_tools() {
        let mut m = Menus::new();
        select(&mut m, 0, Item::Menu(MenuId::Tools));
        m.handle(Nav::Enter, 0, ROWS);
        for (it, pg) in [(Item::WifiChannels, Page::WifiChannels), (Item::Hotspot, Page::Hotspot), (Item::DropBox, Page::Server(false)), (Item::Opds, Page::Server(true))] {
            select(&mut m, 0, it);
            assert_eq!(m.handle(Nav::Enter, 0, ROWS), Effect::Opened(pg));
            m.handle(Nav::Back, 0, ROWS);
        }
        // OK toggles the hotspot / servers, Left goes back
        select(&mut m, 0, Item::Hotspot);
        m.handle(Nav::Enter, 0, ROWS);
        assert_eq!(m.handle(Nav::Enter, 0, ROWS), Effect::Activate(Page::Hotspot));
        m.handle(Nav::Back, 0, ROWS);
        // saved networks: OK switches only on a real row
        select(&mut m, 0, Item::WifiSaved);
        m.handle(Nav::Enter, 0, ROWS);
        assert_eq!(m.handle(Nav::Enter, 0, (0, 5)), Effect::None);
        assert_eq!(m.handle(Nav::Enter, 0, (2, 5)), Effect::Activate(Page::WifiSaved));
        m.handle(Nav::Back, 0, ROWS);
        // meter from the menu follows the connected network
        select(&mut m, 0, Item::WifiSignal);
        assert_eq!(m.handle(Nav::Enter, 0, ROWS), Effect::Opened(Page::WifiSignal));
        assert_eq!(m.list().sel, usize::MAX);
        m.handle(Nav::Back, 0, ROWS);
        // scanner > details > OK = meter for that row
        select(&mut m, 0, Item::WifiScan);
        m.handle(Nav::Enter, 0, ROWS);
        m.handle(Nav::Down, 0, (3, 5));
        m.handle(Nav::Enter, 0, (3, 5));
        assert_eq!(m.page(), Page::ScanDetail);
        assert_eq!(m.handle(Nav::Enter, 0, ROWS), Effect::Opened(Page::WifiSignal));
        assert_eq!(m.list().sel, 1);
        m.handle(Nav::Back, 0, ROWS);
        m.handle(Nav::Back, 0, ROWS);
        assert_eq!(m.page(), Page::WifiScan);
    }

    #[test]
    fn m6_notice_ok_dismisses() {
        let mut m = Menus::new();
        m.push(Page::Notice);
        assert_eq!(m.handle(Nav::Enter, 0, ROWS), Effect::Activate(Page::Notice));
        assert_eq!(m.page(), Page::Menu(MenuId::Top));
    }

    #[test]
    fn transfer_ok_rescans() {
        let mut m = Menus::new();
        select(&mut m, 0, Item::Transfer);
        m.handle(Nav::Enter, 0, ROWS);
        assert_eq!(m.handle(Nav::Enter, 0, ROWS), Effect::RescanMusic);
    }
}
