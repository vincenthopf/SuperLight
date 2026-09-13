#![forbid(unsafe_code)]

pub mod actions;
pub mod config;
pub mod devices;
pub mod gesture;
pub mod hidpp;
pub mod input;
pub mod policy;
pub mod queue;
pub mod reports;
pub mod session;

pub const BASELINE_REVISION: &str = "34d93f70b2a84e425698e0d3d748cae2c5d18911";
pub const CONFIG_LIMIT: usize = 1_048_576;
