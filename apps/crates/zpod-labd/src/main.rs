//! zpod-labd: root helper for the ZPOD lab apps (see lib.rs for the
//! protocol). Started by /etc/init.d/S50zpod-labd; logs to stderr.

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
fn main() {
    linux::main();
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("zpod-labd: Linux only");
    std::process::exit(1);
}
