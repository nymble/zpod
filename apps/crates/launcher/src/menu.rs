//! Launcher menu tree and navigation (pure logic, unit-tested).
//!
//! Conventions (docs/ui/BUTTONS-AND-MENUS.md): Up/Down move, Right/OK/Circle
//! enter, Left/X back one level, Home = top menu, Play/Pause = music,
//! Vol+/- = volume (overlay drawn by the platform layer). Volume is NOT a
//! menu item, and no button is dedicated to an app (Clock is a menu entry).
//!
//! test3 layout: Top = apps, Tools >, Settings >, Power >. Tools = Wi-Fi,
//! Bluetooth, USB, Battery, File transfer. Wi-Fi is one screen: status card
//! plus rows that depend on the mode (Off / Client / Hotspot), with a Wi-Fi
//! tools submenu (channel use, devices, drop box, book catalog).
//! Play/Pause toggles music on the top menu only; elsewhere it only pauses
//! music that is actually playing (never starts it, never "nothing queued").
//!
//! Menu tree (core 0.10.0, docs/APP-MANIFEST.md): apps say where they live
//! (`[menu] path`); [`zpod_apps::tree::Tree`] places them. Home = apps and
//! folders placed at Home, then Tools, Settings, Power. Tools and Settings
//! mix their built-in items with the apps placed there, by order. A folder
//! is `MenuId::Folder(node)`; Left/X goes back one level, Home to the top.

use zpod_apps::tree::{Entry, Tree};
use zpod_core::Nav;
use zpod_ui::widgets::ListState;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuId {
    Top,
    /// a folder of the app tree (index into `Tree::nodes`), e.g. Games
    Folder(u16),
    Settings,
    Display,
    Diagnostics,
    Tools,
    WifiTools,
    Power,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    /// Installed app, index into the launcher's app list.
    App(usize),
    /// Folder of the app tree (index into `Tree::nodes`).
    Folder(u16),
    /// Tools > Updates: load core updates from /data at boot (on/off)
    Updates,
    Transfer,
    Battery,
    Wifi,
    Usb,
    WifiChannels,
    WifiDevices,
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
    /// test3: Wi-Fi status card + mode-dependent rows ([`wifi_rows`])
    Wifi,
    /// Off / Client / Hotspot chooser
    WifiMode,
    /// networks nearby (scan); OK = network details
    WifiScan,
    /// network details with the live signal meter (scan row, saved network,
    /// or the connected one); OK = join
    WifiSignal,
    /// Wi-Fi tools > channel utilization chart
    WifiChannels,
    /// Wi-Fi tools > devices & addresses (APs heard + LAN neighbours)
    WifiDevices,
    /// saved networks: OK = details, Tri twice = forget
    WifiSaved,
    /// type the password of a secured network on a USB keyboard, then join
    WifiPassword,
    /// hotspot name + password (OK = show/hide, Tri = new password)
    Hotspot,
    /// devices connected to the hotspot
    HotspotDevices,
    /// USB port: role now (computer link / USB devices), devices, and the
    /// switch rows (Up/Down + OK)
    Usb,
    /// "Switch USB?" confirmation for one target
    UsbConfirm(UsbChoice),
    /// M6: web drop box / OPDS catalog on/off (true = OPDS)
    Server(bool),
    /// M6: first-boot files applied after the first boot (shown at start)
    Notice,
    /// Tools > Updates: boot option, active core, last boot result
    Updates,
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

/// Tools > USB rows, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UsbChoice {
    Host,
    Gadget,
    Auto,
}

impl UsbChoice {
    pub const ALL: [UsbChoice; 3] = [UsbChoice::Host, UsbChoice::Gadget, UsbChoice::Auto];
    pub fn target(self) -> zpod_apps::ctl::UsbTarget {
        match self {
            UsbChoice::Host => zpod_apps::ctl::UsbTarget::Host,
            UsbChoice::Gadget => zpod_apps::ctl::UsbTarget::Gadget,
            UsbChoice::Auto => zpod_apps::ctl::UsbTarget::Auto,
        }
    }
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
    /// Wi-Fi tools: OK on a page that does something (switch network,
    /// hotspot on/off, server on/off, open the meter from scan details).
    Activate(Page),
    /// File transfer: OK = rescan the music library now.
    RescanMusic,
    /// Entered a page that loads data (Wi-Fi info, apps list, date editor).
    Opened(Page),
    /// Play/Pause away from the top menu: pause only if music is playing.
    PauseIfPlaying,
    Power(bool),
}

fn tree_items(t: &Tree, node: Option<usize>) -> Vec<(i32, Item)> {
    let Some(n) = node.filter(|n| *n < t.nodes.len()) else { return Vec::new() };
    t.nodes[n]
        .entries
        .iter()
        .map(|e| {
            let it = match e {
                Entry::App { app, .. } => Item::App(*app),
                Entry::Folder(f) => Item::Folder(*f as u16),
            };
            (t.order_of(e), it)
        })
        .collect()
}

/// Built-in items with their order, merged with the apps placed in the
/// same folder (stable: a built-in item comes first on a tie).
fn merged(builtin: &[(i32, Item)], t: &Tree, path: &str) -> Vec<Item> {
    let mut v: Vec<(i32, usize, Item)> = builtin.iter().enumerate().map(|(i, (o, it))| (*o, i, *it)).collect();
    let n = builtin.len();
    v.extend(tree_items(t, t.find(path)).into_iter().enumerate().map(|(i, (o, it))| (o, n + i, it)));
    v.sort_by_key(|(o, i, _)| (*o, *i));
    v.into_iter().map(|(_, _, it)| it).collect()
}

/// Order of the built-in Tools items (apps placed in Tools sort among them;
/// Tools/Radio is 60).
pub const TOOLS_BUILTIN: [(i32, Item); 6] =
    [(10, Item::Wifi), (20, Item::BtScan), (30, Item::Usb), (40, Item::Battery), (50, Item::Transfer), (80, Item::Updates)];

pub fn items(m: MenuId, t: &Tree) -> Vec<Item> {
    match m {
        MenuId::Top => {
            let mut v: Vec<Item> = tree_items(t, Some(0)).into_iter().map(|(_, it)| it).collect();
            v.extend([Item::Menu(MenuId::Tools), Item::Menu(MenuId::Settings), Item::Menu(MenuId::Power)]);
            v
        }
        MenuId::Folder(f) => tree_items(t, Some(f as usize)).into_iter().map(|(_, it)| it).collect(),
        MenuId::Settings => merged(
            &[(0, Item::Menu(MenuId::Display)), (0, Item::DateTime), (0, Item::About), (0, Item::Apps), (0, Item::Menu(MenuId::Diagnostics))],
            t,
            "Settings",
        ),
        MenuId::Display => vec![Item::BacklightTest, Item::SpiSpeed],
        MenuId::Diagnostics => vec![Item::Status, Item::Storage, Item::ButtonTest, Item::Tone],
        MenuId::Tools => merged(&TOOLS_BUILTIN, t, "Tools"),
        MenuId::WifiTools => vec![Item::WifiChannels, Item::WifiDevices, Item::DropBox, Item::Opds],
        MenuId::Power => vec![Item::Reboot, Item::Shutdown],
    }
}

/// The tree node a menu page shows apps of (Home, Tools, Settings, folders).
pub fn node_of(m: MenuId, t: &Tree) -> Option<usize> {
    match m {
        MenuId::Top => Some(0),
        MenuId::Folder(f) => Some(f as usize),
        MenuId::Tools => t.find("Tools"),
        MenuId::Settings => t.find("Settings"),
        _ => None,
    }
}

/// (title, subtitle) for built-in items; apps are labelled by the launcher.
pub fn label(it: Item) -> (&'static str, &'static str) {
    match it {
        Item::App(_) | Item::Folder(_) => ("", ""),
        Item::Updates => ("Updates", "Load updates from /data at boot, versions"),
        Item::Transfer => ("File transfer", "How to connect, free space, rescan"),
        Item::Battery => ("Battery", "Charge, time left, last charge, history"),
        Item::Wifi => ("Wi-Fi", "Off, client or hotspot; networks, devices"),
        Item::Usb => ("USB", "Computer link or USB devices"),
        Item::WifiChannels => ("Channel utilization", "2.4 GHz networks and the least busy channel"),
        Item::WifiDevices => ("Devices & addresses", "Access points heard, LAN neighbours"),
        Item::DropBox => ("Drop box", "Web page: browse, download, upload"),
        Item::Opds => ("Book catalog", "OPDS feed of /data/books for e-readers"),
        Item::BtScan => ("Bluetooth", "Classic + BLE devices nearby, RSSI"),
        Item::Menu(MenuId::Tools) => ("Tools", "Wi-Fi, Bluetooth, USB, radio, updates"),
        Item::Menu(MenuId::Folder(_)) => ("", ""),
        Item::Menu(MenuId::WifiTools) => ("Wi-Fi tools", "Channels, devices, drop box, books"),
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
        // the launcher titles folders from the tree
        Page::Menu(MenuId::Folder(_)) => "",
        Page::Updates => "Updates",
        Page::Menu(MenuId::Settings) => "Settings",
        Page::Menu(MenuId::Display) => "Display",
        Page::Menu(MenuId::Diagnostics) => "Diagnostics",
        Page::Menu(MenuId::Power) => "Power",
        Page::Menu(MenuId::Tools) => "Tools",
        Page::Menu(MenuId::WifiTools) => "Wi-Fi tools",
        Page::WifiMode => "Wi-Fi mode",
        Page::WifiScan => "Networks",
        Page::Battery => "Battery",
        Page::WifiSignal => "Network",
        Page::WifiChannels => "Channel use",
        Page::WifiDevices => "Devices",
        Page::WifiSaved => "Saved networks",
        Page::WifiPassword => "Join network",
        Page::Hotspot => "Hotspot",
        Page::HotspotDevices => "Connected",
        Page::Usb | Page::UsbConfirm(_) => "USB",
        Page::Server(false) => "Drop box",
        Page::Server(true) => "Books",
        Page::Notice => "Setup files",
        Page::BtScan => "Bluetooth",
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

/// Rows under the Wi-Fi status card, by mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WifiRow {
    Mode,
    /// client: scan and join
    Networks,
    /// client: saved networks (details, forget)
    Saved,
    /// hotspot: connected devices
    Devices,
    /// hotspot: show / change password
    Password,
    /// Wi-Fi tools submenu
    Tools,
}

pub fn wifi_rows(mode: zpod_hal::wifi::Mode) -> Vec<WifiRow> {
    use zpod_hal::wifi::Mode;
    match mode {
        Mode::Off => vec![WifiRow::Mode, WifiRow::Tools],
        Mode::Client => vec![WifiRow::Mode, WifiRow::Networks, WifiRow::Saved, WifiRow::Tools],
        Mode::Hotspot => vec![WifiRow::Mode, WifiRow::Devices, WifiRow::Password, WifiRow::Tools],
    }
}

impl Menus {
    /// Open the page behind a Wi-Fi row (the launcher calls this for
    /// Effect::Activate(Page::Wifi) once it knows the mode).
    pub fn open_wifi_row(&mut self, r: WifiRow) -> Effect {
        let p = match r {
            WifiRow::Mode => Page::WifiMode,
            WifiRow::Networks => Page::WifiScan,
            WifiRow::Saved => Page::WifiSaved,
            WifiRow::Devices => Page::HotspotDevices,
            WifiRow::Password => Page::Hotspot,
            WifiRow::Tools => Page::Menu(MenuId::WifiTools),
        };
        self.push(p);
        Effect::Opened(p)
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
    pub fn selected(&self, t: &Tree) -> Option<Item> {
        match self.page() {
            Page::Menu(m) => items(m, t).get(self.list().sel).copied(),
            _ => None,
        }
    }

    /// Conventional handling for menus and simple pages. Pages with their
    /// own bindings (DateTime editor, ButtonTest) are handled by the caller.
    /// `rows` = (number of rows on this page, rows visible).
    pub fn handle(&mut self, nav: Nav, t: &Tree, rows: (usize, usize)) -> Effect {
        match nav {
            Nav::Home => {
                self.home();
                return Effect::Redraw;
            }
            Nav::PlayPause => {
                return if self.page() == Page::Menu(MenuId::Top) { Effect::PlayPause } else { Effect::PauseIfPlaying };
            }
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
                    Page::Menu(m) => (items(m, t).len(), rows.1),
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
                    let Some(it) = items(m, t).get(self.list().sel).copied() else { return Effect::None };
                    self.enter(it)
                }
                Page::Wifi | Page::WifiMode if self.list().sel < rows.0 => Effect::Activate(page),
                Page::Transfer => Effect::RescanMusic,
                Page::WifiScan | Page::WifiSaved if self.list().sel < rows.0 => {
                    let sel = self.list().sel;
                    self.push(Page::WifiSignal);
                    self.list_mut().sel = sel; // the row the details are for
                    Effect::Opened(Page::WifiSignal)
                }
                Page::WifiSignal | Page::Hotspot | Page::Server(_) | Page::HotspotDevices | Page::WifiDevices | Page::Usb | Page::Updates => Effect::Activate(page),
                Page::Notice => {
                    self.back();
                    Effect::Activate(Page::Notice)
                }
                Page::BtScan if self.list().sel < rows.0 => {
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
            Item::Folder(f) => open(self, Page::Menu(MenuId::Folder(f))),
            Item::Updates => open(self, Page::Updates),
            Item::Menu(m) => open(self, Page::Menu(m)),
            Item::Transfer => open(self, Page::Transfer),
            Item::Battery => open(self, Page::Battery),
            Item::Wifi => open(self, Page::Wifi),
            Item::Usb => open(self, Page::Usb),
            Item::WifiChannels => open(self, Page::WifiChannels),
            Item::WifiDevices => open(self, Page::WifiDevices),
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

    fn app(id: &str, order: i32, extra: &str) -> zpod_apps::AppInfo {
        let text = format!("schema = 1\n[app]\nid = \"{id}\"\nname = \"{id}\"\nversion = \"1.0.0\"\nexec = \"bin/x\"\nmin_core = \"0.5.0\"\norder = {order}\n{extra}");
        zpod_apps::AppInfo { dir: "/x".into(), manifest: zpod_apps::Manifest::parse(&text).unwrap() }
    }

    /// `home` apps at Home, then `tools` apps placed in Tools (app indices in that order)
    fn tree_with(home: usize, tools: usize) -> Tree {
        let mut v: Vec<_> = (0..home).map(|i| app(&format!("a{i}"), i as i32, "")).collect();
        v.extend((0..tools).map(|i| app(&format!("t{i}"), 100 + i as i32, "[menu]\npath = \"Tools\"\n")));
        Tree::build(&v)
    }

    fn t(n: usize) -> Tree {
        tree_with(n, 0)
    }

    fn select(m: &mut Menus, tr: &Tree, it: Item) {
        let Page::Menu(id) = m.page() else { panic!("not a menu") };
        let i = items(id, tr).iter().position(|x| *x == it).expect("item in menu");
        m.list_mut().sel = i;
    }

    #[test]
    fn no_volume_item_and_tone_only_in_diagnostics() {
        for id in [MenuId::Top, MenuId::Settings, MenuId::Display, MenuId::Diagnostics, MenuId::Tools, MenuId::WifiTools, MenuId::Power] {
            for it in items(id, &t(3)) {
                assert!(!label(it).0.to_lowercase().contains("volume"), "{it:?}");
            }
        }
        assert!(!items(MenuId::Top, &t(3)).contains(&Item::Tone));
        assert!(!items(MenuId::Settings, &t(3)).contains(&Item::Tone));
        assert!(items(MenuId::Diagnostics, &t(3)).contains(&Item::Tone));
    }

    #[test]
    fn left_is_back_one_level_everywhere_and_right_enters() {
        let mut m = Menus::new();
        select(&mut m, &t(2), Item::Menu(MenuId::Settings));
        assert_eq!(m.handle(Nav::Enter, &t(2), ROWS), Effect::Opened(Page::Menu(MenuId::Settings)));
        select(&mut m, &t(2), Item::Menu(MenuId::Diagnostics));
        m.handle(Nav::Enter, &t(2), ROWS);
        assert_eq!(m.page(), Page::Menu(MenuId::Diagnostics));
        assert_eq!(m.depth(), 3);
        m.handle(Nav::Back, &t(2), ROWS);
        assert_eq!(m.page(), Page::Menu(MenuId::Settings));
        m.handle(Nav::Back, &t(2), ROWS);
        assert_eq!(m.page(), Page::Menu(MenuId::Top));
        m.handle(Nav::Back, &t(2), ROWS); // no-op at the top
        assert_eq!(m.depth(), 1);
        select(&mut m, &t(2), Item::Menu(MenuId::Tools));
        m.handle(Nav::Enter, &t(2), ROWS);
        select(&mut m, &t(2), Item::Wifi);
        m.handle(Nav::Enter, &t(2), ROWS);
        assert_eq!(m.page(), Page::Wifi);
        assert_eq!(m.handle(Nav::Enter, &t(2), (4, 4)), Effect::Activate(Page::Wifi));
        m.handle(Nav::Back, &t(2), ROWS);
        assert_eq!(m.page(), Page::Menu(MenuId::Tools));
        m.handle(Nav::Back, &t(2), ROWS);
        assert_eq!(m.page(), Page::Menu(MenuId::Top));
    }

    #[test]
    fn home_always_returns_to_top() {
        let mut m = Menus::new();
        select(&mut m, &t(0), Item::Menu(MenuId::Settings));
        m.handle(Nav::Enter, &t(0), ROWS);
        select(&mut m, &t(0), Item::Menu(MenuId::Display));
        m.handle(Nav::Enter, &t(0), ROWS);
        assert_eq!(m.depth(), 3);
        assert_eq!(m.handle(Nav::Home, &t(0), ROWS), Effect::Redraw);
        assert_eq!(m.page(), Page::Menu(MenuId::Top));
        assert_eq!(m.depth(), 1);
    }

    #[test]
    fn play_pause_toggles_on_top_menu_only_never_tone() {
        let mut m = Menus::new();
        assert_eq!(m.handle(Nav::PlayPause, &t(1), ROWS), Effect::PlayPause);
        select(&mut m, &t(1), Item::Menu(MenuId::Settings));
        m.handle(Nav::Enter, &t(1), ROWS);
        select(&mut m, &t(1), Item::Menu(MenuId::Diagnostics));
        m.handle(Nav::Enter, &t(1), ROWS);
        assert_eq!(m.handle(Nav::PlayPause, &t(1), ROWS), Effect::PauseIfPlaying);
        select(&mut m, &t(1), Item::Tone);
        assert_eq!(m.handle(Nav::Enter, &t(1), ROWS), Effect::Tone);
        // tool screens (test2 bug: Play said "nothing queued" in Tools)
        m.home();
        select(&mut m, &t(1), Item::Menu(MenuId::Tools));
        m.handle(Nav::Enter, &t(1), ROWS);
        assert_eq!(m.handle(Nav::PlayPause, &t(1), ROWS), Effect::PauseIfPlaying);
        select(&mut m, &t(1), Item::Wifi);
        m.handle(Nav::Enter, &t(1), ROWS);
        assert_eq!(m.handle(Nav::PlayPause, &t(1), ROWS), Effect::PauseIfPlaying);
        assert_eq!(m.page(), Page::Wifi);
    }

    #[test]
    fn apps_launch_and_confirm_power() {
        let mut m = Menus::new();
        assert_eq!(m.handle(Nav::Enter, &t(3), ROWS), Effect::Launch(0));
        m.handle(Nav::Down, &t(3), ROWS);
        assert_eq!(m.handle(Nav::Enter, &t(3), ROWS), Effect::Launch(1));
        select(&mut m, &t(3), Item::Menu(MenuId::Power));
        m.handle(Nav::Enter, &t(3), ROWS);
        select(&mut m, &t(3), Item::Shutdown);
        m.handle(Nav::Enter, &t(3), ROWS);
        assert_eq!(m.page(), Page::Confirm(false));
        assert_eq!(m.handle(Nav::Up, &t(3), ROWS), Effect::None);
        assert_eq!(m.handle(Nav::Back, &t(3), ROWS), Effect::Redraw);
        assert_eq!(m.page(), Page::Menu(MenuId::Power));
        select(&mut m, &t(3), Item::Reboot);
        m.handle(Nav::Enter, &t(3), ROWS);
        assert_eq!(m.handle(Nav::Enter, &t(3), ROWS), Effect::Power(true));
    }

    #[test]
    fn storage_and_build_in_diagnostics() {
        let mut m = Menus::new();
        select(&mut m, &t(0), Item::Menu(MenuId::Settings));
        m.handle(Nav::Enter, &t(0), ROWS);
        select(&mut m, &t(0), Item::Menu(MenuId::Diagnostics));
        m.handle(Nav::Enter, &t(0), ROWS);
        select(&mut m, &t(0), Item::Storage);
        assert_eq!(m.handle(Nav::Enter, &t(0), ROWS), Effect::Opened(Page::Storage));
        assert_eq!(title(m.page()), "Storage");
        m.handle(Nav::Back, &t(0), ROWS);
        assert_eq!(m.page(), Page::Menu(MenuId::Diagnostics));
    }

    #[test]
    fn up_down_wrap_in_menus() {
        let mut m = Menus::new();
        m.handle(Nav::Up, &t(0), ROWS);
        assert_eq!(m.selected(&t(0)), Some(Item::Menu(MenuId::Power)));
        m.handle(Nav::Down, &t(0), ROWS);
        assert_eq!(m.selected(&t(0)), Some(Item::Menu(MenuId::Tools)));
    }

    #[test]
    fn volume_and_app_local_keys_do_nothing_in_menus() {
        let mut m = Menus::new();
        assert_eq!(m.handle(Nav::Volume, &t(0), ROWS), Effect::None);
        assert_eq!(m.handle(Nav::Other(zpod_core::Key::Triangle), &t(0), ROWS), Effect::None);
        assert_eq!(m.handle(Nav::Other(zpod_core::Key::Square), &t(0), ROWS), Effect::None);
        assert_eq!(m.depth(), 1);
    }

    #[test]
    fn test3_top_and_tools_layout() {
        assert_eq!(items(MenuId::Top, &t(1)), vec![Item::App(0), Item::Menu(MenuId::Tools), Item::Menu(MenuId::Settings), Item::Menu(MenuId::Power)]);
        assert_eq!(items(MenuId::Tools, &t(0)), vec![Item::Wifi, Item::BtScan, Item::Usb, Item::Battery, Item::Transfer, Item::Updates]);
        let tools = tree_with(1, 1);
        assert_eq!(items(MenuId::Top, &tools), vec![Item::App(0), Item::Menu(MenuId::Tools), Item::Menu(MenuId::Settings), Item::Menu(MenuId::Power)]);
        assert_eq!(items(MenuId::Tools, &tools).last(), Some(&Item::App(1)), "tool apps (order 100) after the built-in items");
        assert_eq!(items(MenuId::WifiTools, &t(0)), vec![Item::WifiChannels, Item::WifiDevices, Item::DropBox, Item::Opds]);
        assert!(items(MenuId::Settings, &t(0)).contains(&Item::About));
        assert_eq!(label(Item::Wifi).0, "Wi-Fi");
        let mut m = Menus::new();
        select(&mut m, &t(0), Item::Menu(MenuId::Tools));
        assert_eq!(m.handle(Nav::Enter, &t(0), ROWS), Effect::Opened(Page::Menu(MenuId::Tools)));
        for (it, pg) in [(Item::Wifi, Page::Wifi), (Item::BtScan, Page::BtScan), (Item::Usb, Page::Usb), (Item::Battery, Page::Battery), (Item::Transfer, Page::Transfer)] {
            select(&mut m, &t(0), it);
            assert_eq!(m.handle(Nav::Enter, &t(0), ROWS), Effect::Opened(pg));
            m.handle(Nav::Back, &t(0), ROWS);
        }
        select(&mut m, &t(0), Item::BtScan);
        m.handle(Nav::Enter, &t(0), ROWS);
        assert_eq!(m.handle(Nav::Enter, &t(0), (0, 5)), Effect::None);
        m.handle(Nav::Down, &t(0), (3, 5));
        assert_eq!(m.handle(Nav::Enter, &t(0), (3, 5)), Effect::Opened(Page::ScanDetail));
        assert_eq!(m.list().sel, 1);
        assert_eq!(m.handle(Nav::Home, &t(0), ROWS), Effect::Redraw);
        assert_eq!(m.page(), Page::Menu(MenuId::Top));
    }

    #[test]
    fn test3_wifi_rows_by_mode() {
        use zpod_hal::wifi::Mode;
        assert_eq!(wifi_rows(Mode::Off), vec![WifiRow::Mode, WifiRow::Tools]);
        assert_eq!(wifi_rows(Mode::Client), vec![WifiRow::Mode, WifiRow::Networks, WifiRow::Saved, WifiRow::Tools]);
        assert_eq!(wifi_rows(Mode::Hotspot), vec![WifiRow::Mode, WifiRow::Devices, WifiRow::Password, WifiRow::Tools]);
        let mut m = Menus::new();
        m.push(Page::Wifi);
        assert_eq!(m.handle(Nav::Enter, &t(0), (0, 4)), Effect::None, "no rows yet");
        for (r, pg) in [
            (WifiRow::Mode, Page::WifiMode),
            (WifiRow::Networks, Page::WifiScan),
            (WifiRow::Saved, Page::WifiSaved),
            (WifiRow::Devices, Page::HotspotDevices),
            (WifiRow::Password, Page::Hotspot),
            (WifiRow::Tools, Page::Menu(MenuId::WifiTools)),
        ] {
            assert_eq!(m.open_wifi_row(r), Effect::Opened(pg));
            assert_eq!(m.page(), pg);
            m.handle(Nav::Back, &t(0), ROWS);
            assert_eq!(m.page(), Page::Wifi);
        }
    }

    #[test]
    fn test3_wifi_pages() {
        let mut m = Menus::new();
        m.push(Page::Wifi);
        m.open_wifi_row(WifiRow::Mode);
        assert_eq!(m.handle(Nav::Enter, &t(0), (3, 5)), Effect::Activate(Page::WifiMode));
        m.back();
        m.open_wifi_row(WifiRow::Networks);
        assert_eq!(m.handle(Nav::Enter, &t(0), (0, 5)), Effect::None);
        m.handle(Nav::Down, &t(0), (3, 5));
        assert_eq!(m.handle(Nav::Enter, &t(0), (3, 5)), Effect::Opened(Page::WifiSignal));
        assert_eq!(m.list().sel, 1);
        assert_eq!(m.handle(Nav::Enter, &t(0), ROWS), Effect::Activate(Page::WifiSignal));
        m.back();
        assert_eq!(m.page(), Page::WifiScan);
        m.back();
        m.open_wifi_row(WifiRow::Saved);
        assert_eq!(m.handle(Nav::Enter, &t(0), (0, 5)), Effect::None);
        assert_eq!(m.handle(Nav::Enter, &t(0), (2, 5)), Effect::Opened(Page::WifiSignal));
        m.back();
        m.back();
        m.open_wifi_row(WifiRow::Password);
        assert_eq!(m.handle(Nav::Enter, &t(0), ROWS), Effect::Activate(Page::Hotspot));
        m.back();
        m.open_wifi_row(WifiRow::Tools);
        for (it, pg) in [(Item::WifiChannels, Page::WifiChannels), (Item::WifiDevices, Page::WifiDevices), (Item::DropBox, Page::Server(false)), (Item::Opds, Page::Server(true))] {
            select(&mut m, &t(0), it);
            assert_eq!(m.handle(Nav::Enter, &t(0), ROWS), Effect::Opened(pg));
            m.handle(Nav::Back, &t(0), ROWS);
        }
        assert_eq!(m.page(), Page::Menu(MenuId::WifiTools));
    }

    #[test]
    fn m6_notice_ok_dismisses() {
        let mut m = Menus::new();
        m.push(Page::Notice);
        assert_eq!(m.handle(Nav::Enter, &t(0), ROWS), Effect::Activate(Page::Notice));
        assert_eq!(m.page(), Page::Menu(MenuId::Top));
    }

    #[test]
    fn transfer_ok_rescans() {
        let mut m = Menus::new();
        m.push(Page::Menu(MenuId::Tools));
        select(&mut m, &t(0), Item::Transfer);
        m.handle(Nav::Enter, &t(0), ROWS);
        assert_eq!(m.handle(Nav::Enter, &t(0), ROWS), Effect::RescanMusic);
    }

    #[test]
    fn nested_folders_enter_back_home() {
        // the bundled apps with their schema-1 placements
        let apps = vec![
            app("music", 10, ""),
            app("clock", 40, ""),
            app("tetris", 10, "[menu]\npath = \"Games\"\n"),
            app("mousemusic", 10, "[menu]\npath = \"Generative\"\n"),
            app("gps", 10, "[menu]\npath = \"Tools/Radio\"\n"),
            app("tool", 45, "[menu]\npath = \"Tools\"\n"),
            app("setting", 1, "[menu]\npath = \"Settings\"\n"),
        ];
        let tr = Tree::build(&apps);
        let gen = tr.find("Generative").unwrap() as u16;
        let games = tr.find("Games").unwrap() as u16;
        let radio = tr.find("Tools/Radio").unwrap() as u16;
        assert_eq!(
            items(MenuId::Top, &tr),
            vec![Item::App(0), Item::Folder(gen), Item::Folder(games), Item::App(1), Item::Menu(MenuId::Tools), Item::Menu(MenuId::Settings), Item::Menu(MenuId::Power)]
        );
        assert_eq!(
            items(MenuId::Tools, &tr),
            vec![Item::Wifi, Item::BtScan, Item::Usb, Item::Battery, Item::App(5), Item::Transfer, Item::Folder(radio), Item::Updates]
        );
        assert_eq!(items(MenuId::Folder(radio), &tr), vec![Item::App(4)]);
        assert_eq!(items(MenuId::Settings, &tr).last(), Some(&Item::App(6)));
        assert_eq!(node_of(MenuId::Folder(games), &tr), Some(games as usize));
        let mut m = Menus::new();
        select(&mut m, &tr, Item::Folder(games));
        assert_eq!(m.handle(Nav::Enter, &tr, ROWS), Effect::Opened(Page::Menu(MenuId::Folder(games))));
        assert_eq!(m.handle(Nav::Enter, &tr, ROWS), Effect::Launch(2));
        assert_eq!(m.handle(Nav::Back, &tr, ROWS), Effect::Redraw);
        assert_eq!(m.page(), Page::Menu(MenuId::Top));
        // Tools > Radio > GPS, Left back one level, Home to the top
        select(&mut m, &tr, Item::Menu(MenuId::Tools));
        m.handle(Nav::Enter, &tr, ROWS);
        select(&mut m, &tr, Item::Folder(radio));
        m.handle(Nav::Enter, &tr, ROWS);
        assert_eq!(m.page(), Page::Menu(MenuId::Folder(radio)));
        assert_eq!(m.depth(), 3);
        assert_eq!(m.handle(Nav::Enter, &tr, ROWS), Effect::Launch(4));
        m.handle(Nav::Back, &tr, ROWS);
        assert_eq!(m.page(), Page::Menu(MenuId::Tools));
        select(&mut m, &tr, Item::Folder(radio));
        m.handle(Nav::Enter, &tr, ROWS);
        assert_eq!(m.handle(Nav::Home, &tr, ROWS), Effect::Redraw);
        assert_eq!(m.depth(), 1);
        // Up/Down wrap inside a folder of one
        m.push(Page::Menu(MenuId::Folder(radio)));
        m.handle(Nav::Down, &tr, ROWS);
        assert_eq!(m.selected(&tr), Some(Item::App(4)));
        // Tools > Updates opens its page, OK toggles
        m.home();
        m.push(Page::Menu(MenuId::Tools));
        select(&mut m, &tr, Item::Updates);
        assert_eq!(m.handle(Nav::Enter, &tr, ROWS), Effect::Opened(Page::Updates));
        assert_eq!(m.handle(Nav::Enter, &tr, ROWS), Effect::Activate(Page::Updates));
        // a stale folder index (tree shrank) shows nothing instead of panicking
        assert!(items(MenuId::Folder(999), &tr).is_empty());
    }
}
