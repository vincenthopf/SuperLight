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

pub const CONFIG_LIMIT: usize = 1_048_576;
