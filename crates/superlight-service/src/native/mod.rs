mod login;
use crate::shared::Shared;
use std::{io, sync::Arc, time::Duration};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub(crate) mod macos_ffi;
#[cfg(windows)]
mod windows;
#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(windows)]
use windows as platform;
#[cfg(target_os = "linux")]
use linux as platform;

pub use platform::{attach_console, chord, error_dialog, foreground, media, mouse, post, scroll, set_start_at_login, system};

#[derive(Clone, Copy, Debug)]
pub enum UiEvent { Quit, Permissions, Refresh, Focus(u32) }

pub fn run(shared: Arc<Shared>, headless: bool) -> io::Result<()> {
    if headless {
        while !shared.stopping() { shared.wait(Duration::from_secs(60)); }
        return Ok(());
    }
    platform::run(shared)
}
