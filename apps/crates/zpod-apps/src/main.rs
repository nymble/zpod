//! zpod-app: install / update / remove / list ZPOD apps without an image rebuild.
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use zpod_apps::package::{self, Mode};

const USAGE: &str = "usage:
  zpod-app list                         installed apps
  zpod-app info <id | file.zpa>         show manifest
  zpod-app install [--force] <file.zpa> install a new app (--force: reinstall/downgrade)
  zpod-app update  [--force] <file.zpa> update an installed app to a newer version
  zpod-app remove  <id>                 uninstall
  zpod-app pack <app-dir> [out-dir]     build <id>-<version>.zpa
  zpod-app check <file.zpa>             validate without installing
env: ZPOD_APPS_DIR (default /opt/zpod/apps)";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("zpod-app: {e}");
            ExitCode::FAILURE
        }
    }
}

fn notify_launcher() {
    // The launcher rescans when the apps dir (or this file) changes. Best effort.
    let _ = std::fs::write("/run/zpod/apps-changed", b"1");
}

fn run(args: &[String]) -> Result<(), String> {
    let apps = zpod_apps::apps_dir();
    let force = args.iter().any(|a| a == "--force" || a == "-f");
    let pos: Vec<&str> = args.iter().map(String::as_str).filter(|a| !a.starts_with('-')).collect();
    let core = zpod_apps::core_version();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("{USAGE}");
        return Ok(());
    }
    match pos.as_slice() {
        ["list"] | [] => {
            let (list, bad) = zpod_apps::discover(&apps);
            if list.is_empty() && bad.is_empty() {
                println!("no apps installed in {}", apps.display());
            }
            for a in &list {
                let m = &a.manifest.app;
                println!("{:<16} {:<10} {:<18} {}", m.id, m.version, m.name, m.description);
            }
            for (id, why) in &bad {
                println!("{id:<16} BROKEN     {why}");
            }
            Ok(())
        }
        ["info", what] => {
            let m = if what.ends_with(".zpa") {
                package::inspect(Path::new(what))?
            } else {
                zpod_apps::AppInfo::load(&apps.join(what))?.manifest
            };
            let a = &m.app;
            println!(
                "id:          {}\nname:        {}\nversion:     {}\nexec:        {} {}\nicon:        {}\ndescription: {}\nmin_core:    {} (device: {core})\norder:       {}",
                a.id, a.name, a.version, a.exec, a.args.join(" "), a.icon.as_deref().unwrap_or("-"), a.description, a.min_core, a.order
            );
            Ok(())
        }
        ["install", pkg] | ["update", pkg] => {
            let mode = if force { Mode::Force } else if pos[0] == "install" { Mode::Install } else { Mode::Update };
            let r = package::install(Path::new(pkg), &apps, mode, core)?;
            for w in &r.warnings {
                eprintln!("warning: {w}");
            }
            match r.from {
                Some(v) => println!("updated {} ({}) {v} -> {}", r.id, r.name, r.to),
                None => println!("installed {} ({}) {}", r.id, r.name, r.to),
            }
            notify_launcher();
            Ok(())
        }
        ["remove", id] => {
            let v = package::remove(&apps, id)?;
            println!("removed {id} {v}");
            notify_launcher();
            Ok(())
        }
        ["pack", dir] | ["pack", dir, _] => {
            let out = pos.get(2).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
            let p = package::pack(Path::new(dir), &out)?;
            println!("{}", p.display());
            Ok(())
        }
        ["check", pkg] => {
            let tmp = std::env::temp_dir().join(format!("zpod-app-check-{}", std::process::id()));
            let r = package::install(Path::new(pkg), &tmp, Mode::Install, core);
            let _ = std::fs::remove_dir_all(&tmp);
            let r = r?;
            for w in &r.warnings {
                eprintln!("warning: {w}");
            }
            println!("ok: {} {} is installable on core {core}", r.id, r.to);
            Ok(())
        }
        _ => Err(USAGE.into()),
    }
}
