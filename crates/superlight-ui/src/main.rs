#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod app;

#[cfg(windows)]
fn main() -> eframe::Result {
    app::main()
}

#[cfg(not(windows))]
fn main() {
    eprintln!(
        "superlight-ui is the Windows settings app. On macOS, build macos/inspector instead."
    );
    std::process::exit(1);
}
