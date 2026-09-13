#[cfg(not(any(target_os = "macos", windows)))]
compile_error!("SuperLight supports macOS and Windows only");

pub mod hardware;
pub mod hid_access;
pub mod hook;
pub mod native;
pub mod output;
pub mod runtime;
pub mod shared;
pub mod transport;

#[cfg(target_os = "macos")]
mod macos_hid;

pub use runtime::{Options, run};
