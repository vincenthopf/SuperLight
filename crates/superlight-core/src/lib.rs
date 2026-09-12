#![forbid(unsafe_code)]

pub mod config;
pub mod devices;
pub mod hidpp;
pub mod queue;

pub const BASELINE_REVISION: &str = "34d93f70b2a84e425698e0d3d748cae2c5d18911";
pub const CONFIG_LIMIT: usize = 1_048_576;
