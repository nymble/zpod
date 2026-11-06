//! The launcher's menu tree: built-in folders, where each app is placed,
//! and the other ways to reach an app (`[[handles]]`). Pure logic shared by
//! the launcher (drawing) and `zpod-app` (install checks, `zpod-app tree`).
//! Spec: docs/APP-MANIFEST.md.
//!
//! ```text
//! Home                     ""            (apps placed at Home, then the folders)
//! |- Generative            "Generative"  music and art that make themselves
//! |- Games                 "Games"
//! |- Tools                 "Tools"       + built-in Wi-Fi, Bluetooth, USB, ...
//! |  `- Radio              "Tools/Radio" GPS, later SDR / APRS
//! `- Settings              "Settings"    + built-in Display, Date & time, ...
//! ```
//!
//! An app may add sub-folders below a built-in folder ("Generative/Art"),
//! at most [`MAX_DEPTH`] levels, but not new top-level folders: Home stays
//! short. Folders without any app in them are not shown.

use crate::manifest::{Manifest, KNOWN_HANDLES, KNOWN_INPUTS, LAUNCHER_API};
use crate::AppInfo;

/// A built-in folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Builtin {
    pub path: &'static str,
    pub title: &'static str,
    pub sub: &'static str,
    /// Position in the parent folder (Home: apps default to 100; Tools
    /// and Settings stay last, the launcher adds them with Power).
    pub order: i32,
    /// Icon name (launcher assets: folder-<icon>.png)
    pub icon: &'static str,
}

pub const FOLDERS: &[Builtin] = &[
    Builtin { path: "", title: "ZPOD", sub: "", order: 0, icon: "home" },
    Builtin { path: "Generative", title: "Generative", sub: "", order: 20, icon: "generative" },
    Builtin { path: "Games", title: "Games", sub: "", order: 30, icon: "games" },
    Builtin { path: "Tools", title: "Tools", sub: "Wi-Fi, Bluetooth, USB, radio, updates", order: 900, icon: "tools" },
    Builtin { path: "Tools/Radio", title: "Radio", sub: "", order: 60, icon: "radio" },
    Builtin { path: "Settings", title: "Settings", sub: "Display, time, about, diagnostics", order: 910, icon: "settings" },
];

/// Folder levels below Home ("Tools/Radio/X" = 3).
pub const MAX_DEPTH: usize = 3;
/// Order of an app that says nothing (and of a folder an app creates).
pub const DEFAULT_ORDER: i32 = 100;

pub fn builtin(path: &str) -> Option<&'static Builtin> {
    FOLDERS.iter().find(|f| f.path.eq_ignore_ascii_case(path))
}

fn valid_segment(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty() && b.len() <= 20 && b[0].is_ascii_alphanumeric() && b.iter().all(|c| c.is_ascii_alphanumeric() || b" &+-".contains(c)) && !s.ends_with(' ')
}

/// Canonical folder path: "tools/radio" -> "Tools/Radio", "/" or "Home" -> "".
/// Built-in folders keep their own spelling; the first level must be one.
pub fn normalise_path(p: &str) -> Result<String, String> {
    let p = p.trim().trim_matches('/');
    if p.is_empty() || p.eq_ignore_ascii_case("home") {
        return Ok(String::new());
    }
    let segs: Vec<&str> = p.split('/').collect();
    if segs.len() > MAX_DEPTH {
        return Err(format!("menu path '{p}' is deeper than {MAX_DEPTH} folders"));
    }
    if let Some(bad) = segs.iter().find(|s| !valid_segment(s)) {
        return Err(format!("menu path '{p}': bad folder name '{bad}' (1..20 of A-Z a-z 0-9 space & + -)"));
    }
    if builtin(segs[0]).is_none() {
        let tops: Vec<&str> = FOLDERS.iter().filter(|f| !f.path.is_empty() && !f.path.contains('/')).map(|f| f.path).collect();
        return Err(format!("menu path '{p}' must start with a built-in folder ({}) or be \"\" (Home)", tops.join(", ")));
    }
    let mut out = String::new();
    for s in segs {
        let cand = if out.is_empty() { s.to_string() } else { format!("{out}/{s}") };
        out = builtin(&cand).map(|b| b.path.to_string()).unwrap_or(cand);
    }
    Ok(out)
}

/// One place an app is listed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placement {
    pub path: String,
    pub order: i32,
    pub label: String,
}

/// Placements of the bundled apps for manifests written before `[menu]`
/// (schema 0: test3 cards keep those until the image updates them).
const LEGACY: &[(&str, &str, i32)] = &[("music", "", 10), ("clock", "", 40), ("tetris", "Games", 10), ("mousemusic", "Generative", 10), ("gps", "Tools/Radio", 10)];

/// Where the manifest puts the app. Errors are for `zpod-app install`; the
/// launcher falls back to Home for a bad path ([`Tree::build`]).
pub fn placements(m: &Manifest) -> Result<Vec<Placement>, String> {
    let a = &m.app;
    let Some(menu) = &m.menu else {
        // schema 0 / no [menu]: category, or the bundled apps' known places
        if m.schema == 0 {
            if let Some((_, path, order)) = LEGACY.iter().find(|(id, _, _)| *id == a.id) {
                return Ok(vec![Placement { path: path.to_string(), order: *order, label: a.name.clone() }]);
            }
        }
        let path = if a.is_tool() { "Tools" } else { "" };
        return Ok(vec![Placement { path: path.into(), order: a.order, label: a.name.clone() }]);
    };
    let main = Placement {
        path: normalise_path(menu.path.as_deref().unwrap_or(if a.is_tool() { "Tools" } else { "" }))?,
        order: menu.order.unwrap_or(a.order),
        label: menu.label.clone().unwrap_or_else(|| a.name.clone()),
    };
    let mut v = vec![main];
    for x in &menu.also {
        let p = Placement { path: normalise_path(&x.path)?, order: x.order.unwrap_or(v[0].order), label: x.label.clone().unwrap_or_else(|| v[0].label.clone()) };
        if v.iter().any(|q| q.path == p.path) {
            return Err(format!("menu.also: '{}' is listed twice", p.path));
        }
        v.push(p);
    }
    Ok(v)
}

fn hex(s: &str, n: usize) -> bool {
    s.len() == n && s.bytes().all(|c| c.is_ascii_hexdigit())
}

/// Install-time check of placement, handles and requirements. Ok = warnings.
pub fn check(m: &Manifest) -> Result<Vec<String>, String> {
    let mut w: Vec<String> = m.unknown.iter().map(|k| format!("unknown manifest key '{k}' (ignored)")).collect();
    placements(m)?;
    if m.schema == 0 && (m.menu.is_some() || !m.handles.is_empty() || m.requires != Default::default()) {
        w.push("[menu]/[[handles]]/[requires] need 'schema = 1' at the top of manifest.toml".into());
    }
    if m.menu.is_some() && m.app.category.is_some() {
        w.push("app.category is replaced by [menu] path (ignored)".into());
    }
    for h in &m.handles {
        if !KNOWN_HANDLES.contains(&h.kind.as_str()) {
            w.push(format!("handles: unknown kind '{}' (ignored; known: {})", h.kind, KNOWN_HANDLES.join(", ")));
            continue;
        }
        if h.matches.is_empty() {
            return Err(format!("handles ({}): 'match' is empty", h.kind));
        }
        for x in &h.matches {
            let ok = match h.kind.as_str() {
                "usb-device" => match x.split_once(':') {
                    Some(("class", c)) => hex(c, 2),
                    Some((v, "*")) => hex(v, 4),
                    Some((v, p)) => hex(v, 4) && hex(p, 4),
                    None => false,
                },
                "serial" => !x.is_empty() && x.len() <= 16 && x.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-'),
                "file" => !x.is_empty() && x.len() <= 10 && x.bytes().all(|c| c.is_ascii_alphanumeric()),
                "input" => KNOWN_INPUTS.contains(&x.as_str()),
                _ => true,
            };
            if !ok {
                let want = match h.kind.as_str() {
                    "usb-device" => "VVVV:PPPP, VVVV:* or class:CC (hex)",
                    "serial" => "a protocol name such as nmea",
                    "file" => "an extension without the dot, such as mid",
                    _ => "one of buttons, keyboard, mouse, touch",
                };
                return Err(format!("handles ({}): bad match '{x}' (want {want})", h.kind));
            }
        }
    }
    for i in &m.requires.input {
        if !KNOWN_INPUTS.contains(&i.as_str()) {
            w.push(format!("requires.input: unknown '{i}' (ignored)"));
        }
    }
    if m.launcher_api() > LAUNCHER_API {
        return Err(format!("{} needs launcher API {}, this ZPOD has {LAUNCHER_API} (update the core)", m.app.id, m.launcher_api()));
    }
    Ok(w)
}

/// An entry in a folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entry {
    /// index into the app list given to [`Tree::build`]
    App { app: usize, order: i32, label: String },
    /// index into [`Tree::nodes`]
    Folder(usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    pub path: String,
    pub title: String,
    pub sub: String,
    pub order: i32,
    pub icon: &'static str,
    pub builtin: bool,
    pub parent: Option<usize>,
    /// sorted by (order, label); folders without apps are left out
    pub entries: Vec<Entry>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tree {
    /// nodes[0] = Home; then the built-in folders in [`FOLDERS`] order;
    /// then folders apps made
    pub nodes: Vec<Node>,
    /// (app id, why): placement fell back to Home, or the app is hidden
    pub notes: Vec<(String, String)>,
    /// apps left out of the menu (index into the app list)
    pub hidden: Vec<usize>,
}

impl Tree {
    pub fn find(&self, path: &str) -> Option<usize> {
        // folders apps make match case-insensitively (first spelling wins)
        self.nodes.iter().position(|n| n.path.eq_ignore_ascii_case(path))
    }

    fn ensure(&mut self, path: &str) -> usize {
        if let Some(i) = self.find(path) {
            return i;
        }
        if path.is_empty() {
            let h = &FOLDERS[0];
            self.nodes.insert(0, Node { path: String::new(), title: h.title.into(), sub: String::new(), order: 0, icon: h.icon, builtin: true, parent: None, entries: Vec::new() });
            return 0;
        }
        let (parent, name) = match path.rsplit_once('/') {
            Some((p, n)) => (p.to_string(), n.to_string()),
            None => (String::new(), path.to_string()),
        };
        let pi = self.ensure(&parent);
        let b = builtin(path);
        self.nodes.push(Node {
            path: path.to_string(),
            title: b.map(|b| b.title.to_string()).unwrap_or(name),
            sub: b.map(|b| b.sub.to_string()).unwrap_or_default(),
            order: b.map(|b| b.order).unwrap_or(DEFAULT_ORDER),
            icon: b.map(|b| b.icon).unwrap_or("folder"),
            builtin: b.is_some(),
            parent: Some(pi),
            entries: Vec::new(),
        });
        self.nodes.len() - 1
    }

    /// Place every app (`apps` in any order; indices refer to it).
    pub fn build(apps: &[AppInfo]) -> Tree {
        let mut t = Tree::default();
        for f in FOLDERS {
            t.ensure(f.path);
        }
        let mut items: Vec<(usize, usize, i32, String)> = Vec::new(); // (node, app, order, label)
        for (i, a) in apps.iter().enumerate() {
            let m = &a.manifest;
            if m.launcher_api() > LAUNCHER_API {
                t.notes.push((m.app.id.clone(), format!("needs launcher API {} (this one: {LAUNCHER_API})", m.launcher_api())));
                t.hidden.push(i);
                continue;
            }
            let ps = placements(m).unwrap_or_else(|e| {
                t.notes.push((m.app.id.clone(), format!("{e}; shown at Home")));
                vec![Placement { path: String::new(), order: m.app.order, label: m.app.name.clone() }]
            });
            for p in ps {
                let n = t.ensure(&p.path);
                items.push((n, i, p.order, p.label));
            }
        }
        // a folder is shown when it (or a folder below it) has an app
        let mut used = vec![false; t.nodes.len()];
        for (n, ..) in &items {
            let mut c = Some(*n);
            while let Some(i) = c {
                used[i] = true;
                c = t.nodes[i].parent;
            }
        }
        for (n, app, order, label) in items {
            t.nodes[n].entries.push(Entry::App { app, order, label });
        }
        for i in 1..t.nodes.len() {
            let p = t.nodes[i].parent.unwrap();
            // Tools and Settings are always on Home (built-in items); the launcher adds them
            if used[i] && !(p == 0 && matches!(t.nodes[i].path.as_str(), "Tools" | "Settings")) {
                t.nodes[p].entries.push(Entry::Folder(i));
            }
        }
        let nodes = t.nodes.clone();
        let key = |e: &Entry| match e {
            Entry::App { order, label, .. } => (*order, label.to_lowercase()),
            Entry::Folder(f) => (nodes[*f].order, nodes[*f].title.to_lowercase()),
        };
        for n in &mut t.nodes {
            n.entries.sort_by_key(|e| key(e));
        }
        t
    }

    /// Order of an entry (for merging with the launcher's built-in items).
    pub fn order_of(&self, e: &Entry) -> i32 {
        match e {
            Entry::App { order, .. } => *order,
            Entry::Folder(f) => self.nodes[*f].order,
        }
    }

    /// Indented outline (zpod-app tree).
    pub fn outline(&self, apps: &[AppInfo]) -> String {
        fn walk(t: &Tree, apps: &[AppInfo], n: usize, depth: usize, out: &mut String) {
            for e in &t.nodes[n].entries {
                match e {
                    Entry::App { app, label, .. } => out.push_str(&format!("{}{label}  ({})\n", "  ".repeat(depth), apps[*app].manifest.app.id)),
                    Entry::Folder(f) => {
                        out.push_str(&format!("{}{}/\n", "  ".repeat(depth), t.nodes[*f].title));
                        walk(t, apps, *f, depth + 1, out);
                    }
                }
            }
        }
        let mut s = String::from("Home\n");
        walk(self, apps, 0, 1, &mut s);
        for b in ["Tools", "Settings"] {
            if let Some(i) = self.find(b) {
                s.push_str(&format!("  {b}/  (+ built-in items)\n"));
                walk(self, apps, i, 2, &mut s);
            }
        }
        for (id, why) in &self.notes {
            s.push_str(&format!("note: {id}: {why}\n"));
        }
        s
    }
}

/// Apps that declare they handle `what` of `kind` (e.g. "usb-device",
/// "067b:2303"), best first (menu order). Matching is case-insensitive;
/// usb-device "VVVV:*" matches any product of that vendor.
pub fn handlers<'a>(apps: &'a [AppInfo], kind: &str, what: &str) -> Vec<&'a AppInfo> {
    let what = what.trim().trim_start_matches('.').to_ascii_lowercase();
    let mut v: Vec<&AppInfo> = apps
        .iter()
        .filter(|a| a.manifest.launcher_api() <= LAUNCHER_API)
        .filter(|a| {
            a.manifest.handles.iter().filter(|h| h.kind == kind).any(|h| {
                h.matches.iter().any(|m| {
                    let m = m.to_ascii_lowercase();
                    m == what || (kind == "usb-device" && m.ends_with(":*") && what.split(':').next() == m.split(':').next())
                })
            })
        })
        .collect();
    v.sort_by_key(|a| a.manifest.app.order);
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn app(text: &str) -> AppInfo {
        AppInfo { dir: PathBuf::from("/x"), manifest: Manifest::parse(text).unwrap() }
    }
    fn legacy(id: &str, name: &str, order: i32, extra: &str) -> AppInfo {
        app(&format!("[app]\nid = \"{id}\"\nname = \"{name}\"\nversion = \"1.0.0\"\nexec = \"bin/{id}\"\nmin_core = \"0.5.0\"\norder = {order}\n{extra}"))
    }
    fn v1(id: &str, menu: &str) -> AppInfo {
        app(&format!("schema = 1\n[app]\nid = \"{id}\"\nname = \"{id}\"\nversion = \"1.0.0\"\nexec = \"bin/x\"\nmin_core = \"0.5.0\"\n{menu}"))
    }
    fn labels(t: &Tree, apps: &[AppInfo], n: usize) -> Vec<String> {
        t.nodes[n]
            .entries
            .iter()
            .map(|e| match e {
                Entry::App { app, .. } => apps[*app].manifest.app.id.clone(),
                Entry::Folder(f) => format!("{}/", t.nodes[*f].title),
            })
            .collect()
    }

    #[test]
    fn paths_normalise_and_validate() {
        assert_eq!(normalise_path("").unwrap(), "");
        assert_eq!(normalise_path("/").unwrap(), "");
        assert_eq!(normalise_path("Home").unwrap(), "");
        assert_eq!(normalise_path("tools/radio").unwrap(), "Tools/Radio");
        assert_eq!(normalise_path("/Generative/Art/").unwrap(), "Generative/Art");
        assert_eq!(normalise_path("games").unwrap(), "Games");
        assert!(normalise_path("Music/Generative").unwrap_err().contains("built-in folder"));
        assert!(normalise_path("Tools/a/b/c").unwrap_err().contains("deeper"));
        assert!(normalise_path("Tools/../x").unwrap_err().contains("bad folder name"));
        assert!(normalise_path("Tools/").is_ok());
        assert!(normalise_path("Tools//x").is_err());
        assert!(normalise_path("Tools/Way too long a folder name").is_err());
    }

    #[test]
    fn legacy_manifests_map() {
        // test3 manifests on a card: no schema, no [menu]
        let apps = vec![
            legacy("music", "Music", 10, ""),
            legacy("clock", "Clock", 20, ""),
            legacy("tetris", "Tetris", 30, ""),
            legacy("mousemusic", "Mouse Music", 40, "permissions = [\"usb-host\"]"),
            legacy("gps", "GPS", 60, "category = \"tool\""),
            legacy("other", "Other", 50, ""),
            legacy("othertool", "Other tool", 50, "category = \"tool\""),
        ];
        let t = Tree::build(&apps);
        assert_eq!(labels(&t, &apps, 0), ["music", "Generative/", "Games/", "clock", "other"]);
        assert_eq!(labels(&t, &apps, t.find("Games").unwrap()), ["tetris"]);
        assert_eq!(labels(&t, &apps, t.find("Generative").unwrap()), ["mousemusic"]);
        assert_eq!(labels(&t, &apps, t.find("Tools").unwrap()), ["othertool", "Radio/"]);
        assert_eq!(labels(&t, &apps, t.find("Tools/Radio").unwrap()), ["gps"]);
        assert!(t.notes.is_empty());
        // the legacy table is for schema 0 only
        let a = v1("tetris", "");
        assert_eq!(placements(&a.manifest).unwrap()[0].path, "");
    }

    #[test]
    fn v1_placement_order_label_also() {
        let apps = vec![
            v1("mm", "[menu]\npath = \"Generative\"\norder = 5\nlabel = \"Mouse Music\"\n[[menu.also]]\npath = \"Generative/Music\"\n"),
            v1("art", "[menu]\npath = \"generative/Art\"\n"),
            v1("art2", "[menu]\npath = \"Generative/ART\"\n"),
            v1("gps", "[menu]\npath = \"Tools/Radio\"\n"),
            v1("sdr", "[menu]\npath = \"Tools/Radio\"\norder = 50\n"),
            v1("home", ""),
        ];
        let t = Tree::build(&apps);
        assert_eq!(labels(&t, &apps, 0), ["Generative/", "home"]);
        let g = t.find("Generative").unwrap();
        assert_eq!(labels(&t, &apps, g), ["mm", "Art/", "Music/"]);
        assert_eq!(labels(&t, &apps, t.find("Generative/Music").unwrap()), ["mm"]);
        assert_eq!(labels(&t, &apps, t.find("Tools/Radio").unwrap()), ["sdr", "gps"]);
        assert_eq!(labels(&t, &apps, t.find("Generative/Art").unwrap()), ["art", "art2"]);
        match &t.nodes[g].entries[0] {
            Entry::App { label, order, .. } => assert_eq!((label.as_str(), *order), ("Mouse Music", 5)),
            e => panic!("{e:?}"),
        }
        assert_eq!(t.nodes[t.find("Generative/Art").unwrap()].icon, "folder");
        // empty built-in folders are not listed
        assert!(!labels(&t, &apps, 0).contains(&"Games/".to_string()));
        assert!(t.outline(&apps).contains("Generative/\n    Mouse Music  (mm)\n"), "{}", t.outline(&apps));
    }

    #[test]
    fn bad_placement_falls_back_to_home_in_the_launcher_and_fails_install() {
        let a = v1("x", "[menu]\npath = \"Nowhere/Else\"\n");
        assert!(check(&a.manifest).unwrap_err().contains("built-in folder"));
        let apps = vec![a];
        let t = Tree::build(&apps);
        assert_eq!(labels(&t, &apps, 0), ["x"]);
        assert!(t.notes[0].1.contains("shown at Home"));
        let dup = v1("y", "[menu]\npath = \"Games\"\n[[menu.also]]\npath = \"games\"\n");
        assert!(check(&dup.manifest).unwrap_err().contains("twice"));
    }

    #[test]
    fn requires_launcher_api_hides_the_app() {
        let a = v1("future", &format!("[requires]\nlauncher_api = {}\n", LAUNCHER_API + 1));
        assert!(check(&a.manifest).unwrap_err().contains("launcher API"));
        let apps = vec![a, v1("now", "[requires]\nlauncher_api = 3\nusb_host = true\ninput = [\"buttons\", \"keyboard\"]\n")];
        let t = Tree::build(&apps);
        assert_eq!(labels(&t, &apps, 0), ["now"]);
        assert_eq!(t.hidden, [0]);
        assert!(apps[1].manifest.requires.usb_host);
    }

    #[test]
    fn handles_validate_and_match() {
        let gps = v1("gps", "[menu]\npath = \"Tools/Radio\"\n[[handles]]\nkind = \"usb-device\"\nmatch = [\"067b:2303\", \"1546:*\"]\nlabel = \"GPS receiver\"\n[[handles]]\nkind = \"serial\"\nmatch = \"nmea\"\n");
        assert!(check(&gps.manifest).unwrap().is_empty());
        let mm = v1("mm", "[[handles]]\nkind = \"file\"\nmatch = [\"mid\", \"MIDI\"]\n[[handles]]\nkind = \"input\"\nmatch = [\"mouse\"]\n[[handles]]\nkind = \"web-scheme\"\nmatch = \"x\"\n");
        let w = check(&mm.manifest).unwrap();
        assert!(w.len() == 1 && w[0].contains("unknown kind 'web-scheme'"), "{w:?}");
        let apps = vec![gps, mm];
        assert_eq!(handlers(&apps, "usb-device", "067B:2303").len(), 1);
        assert_eq!(handlers(&apps, "usb-device", "1546:01a8")[0].manifest.app.id, "gps");
        assert!(handlers(&apps, "usb-device", "0403:6001").is_empty());
        assert_eq!(handlers(&apps, "serial", "nmea")[0].manifest.app.id, "gps");
        assert_eq!(handlers(&apps, "file", ".midi")[0].manifest.app.id, "mm");
        assert_eq!(handlers(&apps, "input", "mouse")[0].manifest.app.id, "mm");
        for bad in ["\"67b:2303\"", "\"067b2303\"", "\"class:2\"", "[]"] {
            let a = v1("b", &format!("[[handles]]\nkind = \"usb-device\"\nmatch = {bad}\n"));
            assert!(check(&a.manifest).is_err(), "{bad}");
        }
        assert!(check(&v1("b", "[[handles]]\nkind = \"usb-device\"\nmatch = \"class:02\"\n").manifest).is_ok());
        assert!(check(&v1("b", "[[handles]]\nkind = \"file\"\nmatch = \"a.b\"\n").manifest).is_err());
    }

    #[test]
    fn unknown_fields_are_ignored_with_a_warning() {
        let a = v1("x", "future_key = 1\n[menu]\npath = \"Games\"\nshape = \"round\"\n[x-vendor]\nfoo = 1\n[requires]\nram_mb = 64\n");
        let w = check(&a.manifest).unwrap();
        for k in ["app.future_key", "menu.shape", "x-vendor", "requires.ram_mb"] {
            assert!(w.iter().any(|s| s.contains(&format!("'{k}'"))), "{k} in {w:?}");
        }
        assert_eq!(placements(&a.manifest).unwrap()[0].path, "Games");
    }

    #[test]
    fn schema_too_new_is_refused_and_schema0_sections_warn() {
        let e = Manifest::parse("schema = 2\n[app]\nid = \"x\"\nname = \"x\"\nversion = \"1.0.0\"\nexec = \"x\"\nmin_core = \"0.5.0\"\n").unwrap_err();
        assert!(e.contains("schema 2 needs a newer ZPOD"), "{e}");
        let a = legacy("x", "X", 1, "[menu]\npath = \"Games\"\n");
        let w = check(&a.manifest).unwrap();
        assert!(w.iter().any(|s| s.contains("schema = 1")), "{w:?}");
        let c = app("schema = 1\n[app]\nid = \"x\"\nname = \"x\"\nversion = \"1.0.0\"\nexec = \"x\"\nmin_core = \"0.5.0\"\ncategory = \"tool\"\n[menu]\npath = \"Games\"\n");
        assert!(check(&c.manifest).unwrap().iter().any(|s| s.contains("category")));
        assert_eq!(placements(&c.manifest).unwrap()[0].path, "Games");
        // [menu] without a path keeps the legacy category
        let d = app("schema = 1\n[app]\nid = \"x\"\nname = \"x\"\nversion = \"1.0.0\"\nexec = \"x\"\nmin_core = \"0.5.0\"\ncategory = \"tool\"\n[menu]\norder = 3\n");
        assert_eq!(placements(&d.manifest).unwrap()[0], Placement { path: "Tools".into(), order: 3, label: "x".into() });
    }
}
