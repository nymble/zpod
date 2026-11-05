//! zpod-app: install / update / remove / list ZPOD apps without an image rebuild.
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;
use zpod_apps::ctl::{self, Request};
use zpod_apps::logs;
use zpod_apps::package::{self, Mode};

const USAGE: &str = "usage:
  zpod-app list                         installed apps
  zpod-app info <id | file.zpa>         show manifest
  zpod-app install [--force] <file.zpa> install a new app (--force: reinstall/downgrade)
  zpod-app update  [--force] <file.zpa> update an installed app to a newer version
  zpod-app remove  [--purge] <id>       uninstall (--purge: also delete its data and logs)
  zpod-app pack <app-dir> [out-dir]     build <id>-<version>.zpa
  zpod-app check <file.zpa>             validate without installing
  zpod-app launch <id>                  start an app on the screen (stops the running one)
  zpod-app restart [<id>]               stop and start again (default: the running app)
  zpod-app stop [<id>]                  back to the home screen
  zpod-app status                       which app is running
  zpod-app logs [-f] [-n N] <id>        the app's stdout/stderr (-f: follow)
launch/restart/stop/status talk to the launcher (root only); see docs/DEV-LOOP.md.
env: ZPOD_APPS_DIR (default /opt/zpod/apps), ZPOD_DATA_DIR, ZPOD_LOG_DIR, ZPOD_SOCKET";

const CTL_TIMEOUT: Duration = Duration::from_secs(15);

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
    let mut pos: Vec<&str> = Vec::new();
    let mut flags: Vec<&str> = Vec::new();
    let mut lines = 30usize;
    let mut it = args.iter().map(String::as_str);
    while let Some(a) = it.next() {
        if a == "-n" || a == "--lines" {
            lines = it.next().and_then(|n| n.parse().ok()).ok_or("-n needs a number")?;
        } else if a.starts_with('-') {
            flags.push(a);
        } else {
            pos.push(a);
        }
    }
    let flag = |f: &[&str]| flags.iter().any(|a| f.contains(a));
    // -f is --force for install/update and --follow for logs
    let force = flag(&["--force", "-f"]) && pos.first() != Some(&"logs");
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
            let purge = flag(&["--purge"]);
            // Stop it first if it is on the screen (best effort: older launchers have no socket).
            if let Ok(ctl::Status::Running { id: r, .. }) = status() {
                if r == *id {
                    let _ = ctl::send(&Request::Stop(Some(id.to_string())), CTL_TIMEOUT);
                }
            }
            match package::remove(&apps, id) {
                Ok(v) => println!("removed {id} {v}"),
                Err(e) if !purge => return Err(e),
                Err(e) => println!("{e}"),
            }
            notify_launcher();
            if purge {
                for p in logs::purge(&logs::data_root(), &logs::log_dir(), id)? {
                    println!("deleted {}", p.display());
                }
            } else if logs::data_root().join(id).exists() {
                println!("kept data in {} (remove --purge {id} deletes it)", logs::data_root().join(id).display());
            }
            Ok(())
        }
        ["launch", id] => ctl_cmd(Request::Launch(id.to_string())),
        ["restart"] => ctl_cmd(Request::Restart(None)),
        ["restart", id] => ctl_cmd(Request::Restart(Some(id.to_string()))),
        ["stop"] => ctl_cmd(Request::Stop(None)),
        ["stop", id] => ctl_cmd(Request::Stop(Some(id.to_string()))),
        ["status"] => {
            match status()? {
                ctl::Status::Idle => println!("idle (home screen)"),
                ctl::Status::Running { id, pid, secs } => println!("running {id} (pid {pid}, {secs} s)"),
            }
            Ok(())
        }
        ["logs", id] => {
            if !zpod_apps::manifest::valid_id(id) {
                return Err(format!("invalid app id '{id}'"));
            }
            let p = logs::log_path(&logs::log_dir(), id);
            if flag(&["-f", "--follow"]) {
                if !p.exists() {
                    eprintln!("(waiting for {})", p.display());
                }
                let out = std::io::stdout();
                logs::follow(&p, lines, &mut out.lock(), &|| logs::reader_present(1)).map_err(|e| e.to_string())
            } else {
                let t = logs::tail(&p, lines).map_err(|e| format!("{}: {e}", p.display()))?;
                print!("{t}");
                Ok(())
            }
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

fn status() -> Result<ctl::Status, String> {
    let r = ctl::send(&Request::Status, CTL_TIMEOUT)?;
    ctl::Status::parse(&format!("ok {r}")).ok_or_else(|| format!("bad status reply '{r}'"))
}

fn ctl_cmd(req: Request) -> Result<(), String> {
    println!("{}", ctl::send(&req, CTL_TIMEOUT)?);
    Ok(())
}
