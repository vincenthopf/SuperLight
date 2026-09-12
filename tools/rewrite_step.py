from pathlib import Path


def write(path, content):
    file = Path(path)
    file.parent.mkdir(parents=True, exist_ok=True)
    file.write_text(content)


write("crates/superlight-service/src/lib.rs", r'''pub mod hardware;
pub mod hook;
pub mod native;
pub mod output;
pub mod runtime;
pub mod shared;
pub mod transport;

pub use runtime::{Options, run};
''')

write("crates/superlight-service/src/runtime.rs", r'''use crate::{hardware, native, output, shared::{Command, Shared}};
use crossbeam_channel::{Receiver, bounded};
use serde_json::{Value, json};
use std::{io, path::PathBuf, process::{Child, Command as ProcessCommand, Stdio}, sync::{Arc, atomic::Ordering}, thread::{self, JoinHandle}, time::{Duration, Instant}};
use superlight_core::{CONFIG_LIMIT, actions::{Action, Platform}, config, policy::{self, Policy}};
use superlight_ipc::{AppInfo, Endpoint, Paths, Request, Response, Server, Store};

#[derive(Clone, Copy, Debug, Default)]
pub struct Options {
    pub background: bool,
    pub force_ui: bool,
    pub headless: bool,
}

struct Workers {
    shared: Arc<Shared>,
    endpoint: Endpoint,
    handles: Vec<JoinHandle<()>>,
}

impl Workers {
    fn spawn(&mut self, name: &'static str, work: impl FnOnce() + Send + 'static) -> io::Result<()> {
        let shared = Arc::clone(&self.shared);
        let handle = thread::Builder::new().name(name.into()).spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work));
            if result.is_err() {
                shared.report(format!("{name} stopped unexpectedly. Input remapping has been disabled."));
                shared.stop();
            }
        })?;
        self.handles.push(handle);
        Ok(())
    }
}

impl Drop for Workers {
    fn drop(&mut self) {
        self.shared.stop();
        self.endpoint.wake();
        for handle in self.handles.drain(..) { let _ = handle.join(); }
    }
}

pub fn run(options: Options) -> io::Result<()> {
    let paths = Paths::discover()?;
    let store = Store::open(paths.clone())?;
    let show_ui = !options.headless && !options.background
        && (options.force_ui || store.first_run || !policy::boolean(&store.value, "start_minimized", true));
    let server = Server::bind(paths)?;
    let (shared, inputs, commands) = Shared::new(store.value.clone(), server.endpoint.instance.clone(), options.headless).map_err(io::Error::other)?;
    {
        let mut snapshot = shared.snapshot.lock().unwrap_or_else(|error| error.into_inner());
        snapshot.revision = store.revision;
        snapshot.notice = store.notice.clone();
    }
    let interrupt = Arc::clone(&shared);
    ctrlc::set_handler(move || interrupt.stop()).map_err(io::Error::other)?;
    let mut workers = Workers { shared: Arc::clone(&shared), endpoint: server.endpoint.clone(), handles: Vec::with_capacity(4) };
    let controller = Controller::new(store, Arc::clone(&shared))?;
    let output_shared = Arc::clone(&shared);
    workers.spawn("superlight-output", move || output::run(output_shared, inputs))?;
    workers.spawn("superlight-controller", move || controller.run(commands, show_ui))?;
    if !options.headless {
        let hardware_shared = Arc::clone(&shared);
        workers.spawn("superlight-hid", move || hardware::run(hardware_shared))?;
    }
    let ipc_shared = Arc::clone(&shared);
    workers.spawn("superlight-ipc", move || serve(server, ipc_shared))?;
    let result = native::run(Arc::clone(&shared), options.headless);
    drop(workers);
    result
}

fn serve(server: Server, shared: Arc<Shared>) {
    while !shared.stopping() {
        let mut connection = match server.accept() {
            Ok(connection) => connection,
            Err(error) => {
                if !shared.stopping() && !matches!(error.kind(), io::ErrorKind::UnexpectedEof | io::ErrorKind::PermissionDenied | io::ErrorKind::TimedOut | io::ErrorKind::InvalidData) {
                    shared.report(format!("Local control: {error}"));
                    shared.wait(Duration::from_millis(50));
                }
                continue;
            }
        };
        let request = std::mem::replace(&mut connection.request, Request::Get);
        let response = if matches!(request, Request::Get) {
            Response::success(shared.status())
        } else {
            let (reply, receiver) = bounded(1);
            if shared.commands.send_timeout(Command::Rpc { request, reply }, Duration::from_millis(250)).is_err() {
                Response::failure("The service is busy or stopping. Retry the operation.")
            } else {
                receiver.recv_timeout(Duration::from_secs(2)).unwrap_or_else(|_| Response::failure("The operation did not finish before its deadline. Refresh status before retrying."))
            }
        };
        let _ = connection.respond(&response);
    }
}

pub fn apply_settings(store: &mut Store, expected_revision: u64, value: Value, mut set_login: impl FnMut(bool) -> io::Result<()>) -> io::Result<()> {
    if expected_revision != store.revision { return Err(io::Error::new(io::ErrorKind::WouldBlock, "Settings changed. Reload before saving.")); }
    let value = config::migrate(value).map_err(io::Error::other)?;
    policy::validate_actions(&value, Platform::current()).map_err(io::Error::other)?;
    if serde_json::to_vec_pretty(&value).map_err(io::Error::other)?.len() > CONFIG_LIMIT { return Err(io::Error::other("Configuration exceeds 1 MiB")); }
    let old_login = policy::boolean(&store.value, "start_at_login", false);
    let new_login = policy::boolean(&value, "start_at_login", false);
    if old_login != new_login { set_login(new_login)?; }
    if let Err(error) = store.apply(expected_revision, value) {
        if old_login != new_login && let Err(rollback) = set_login(old_login) {
            return Err(io::Error::other(format!("{error}. Restoring the previous login setting also failed: {rollback}")));
        }
        return Err(error);
    }
    Ok(())
}

pub fn device_action(value: &Value, action: Action, dpi_min: u16, dpi_max: u16) -> Result<Value, String> {
    let mut value = value.clone();
    match action {
        Action::ToggleSmartShift => {
            value["settings"]["smart_shift_enabled"] = json!(!policy::boolean(&value, "smart_shift_enabled", false));
        }
        Action::SwitchScrollMode => {
            let mut state = policy::smart_shift(&value);
            state.switch_mode();
            value["settings"]["smart_shift_mode"] = json!(state.mode);
            value["settings"]["smart_shift_enabled"] = json!(false);
        }
        Action::CycleDpi => {
            let candidates = value["settings"]["dpi_presets"].as_array().cloned().unwrap_or_else(|| vec![json!(800), json!(1200), json!(1600), json!(2400)]);
            let mut presets = Vec::with_capacity(candidates.len().min(64));
            for dpi in candidates.iter().take(64).filter_map(Value::as_i64) {
                let dpi = dpi.clamp(i64::from(dpi_min), i64::from(dpi_max.max(dpi_min))) as u16;
                if !presets.contains(&dpi) { presets.push(dpi); }
            }
            if presets.is_empty() { return Err("DPI presets must contain at least one number".into()); }
            let current = policy::number(&value, "dpi", 1000.0) as u16;
            let index = presets.iter().position(|dpi| *dpi == current).map_or(0, |index| (index + 1) % presets.len());
            value["settings"]["dpi"] = json!(presets[index]);
        }
        _ => return Err("Not a device action".into()),
    }
    Ok(value)
}

struct Controller {
    store: Store,
    shared: Arc<Shared>,
    foreground: AppInfo,
    active_profile: String,
    paused: bool,
    ui: Option<Child>,
    executable: PathBuf,
}

impl Controller {
    fn new(store: Store, shared: Arc<Shared>) -> io::Result<Self> {
        Ok(Self { store, shared, foreground: AppInfo::default(), active_profile: "default".into(), paused: false, ui: None, executable: std::env::current_exe()? })
    }

    fn run(mut self, receiver: Receiver<Command>, show_ui: bool) {
        self.refresh_foreground();
        self.publish_configuration();
        if show_ui && let Err(error) = self.show_settings() { self.record_error(error.to_string()); }
        let mut last_poll = Instant::now();
        while !self.shared.stopping() {
            let interval = if self.store.value["profiles"].as_object().is_some_and(|profiles| profiles.len() > 1) { Duration::from_millis(500) } else { Duration::from_secs(2) };
            let remaining = interval.saturating_sub(last_poll.elapsed());
            match receiver.recv_timeout(remaining) {
                Ok(command) => self.handle(command),
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            }
            if last_poll.elapsed() >= interval {
                self.refresh_foreground();
                if let Some(child) = self.ui.as_mut() && matches!(child.try_wait(), Ok(Some(_))) { self.ui = None; }
                last_poll = Instant::now();
            }
        }
    }

    fn record_error(&self, error: String) {
        let error: String = error.chars().take(512).collect();
        let mut snapshot = self.shared.snapshot.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if snapshot.errors.last() == Some(&error) { return; }
        if snapshot.errors.len() == 16 { snapshot.errors.remove(0); }
        snapshot.errors.push(error);
    }

    fn refresh_foreground(&mut self) {
        if self.shared.headless { return; }
        let foreground = native::foreground();
        let restricted = foreground.input_restricted;
        if self.shared.restricted.swap(restricted, Ordering::AcqRel) != restricted && restricted { self.shared.release_all(); }
        if foreground != self.foreground {
            self.foreground = foreground;
            let profile = config::profile_for_aliases(&self.store.value, &self.foreground.aliases).to_owned();
            if profile != self.active_profile {
                self.active_profile = profile;
                self.publish_policy();
            }
            let mut snapshot = self.shared.snapshot.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            snapshot.foreground = self.foreground.clone();
        }
    }

    fn publish_policy(&self) {
        match Policy::compile(&self.store.value, &self.active_profile, Platform::current(), self.paused) {
            Ok(policy) => {
                self.shared.policy.store(Arc::new(policy));
                self.shared.generation.fetch_add(1, Ordering::AcqRel);
                self.shared.wake_hid();
                let mut snapshot = self.shared.snapshot.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                snapshot.active_profile.clone_from(&self.active_profile);
                snapshot.paused = self.paused;
            }
            Err(error) => { self.record_error(error); self.shared.release_all(); }
        }
    }

    fn publish_configuration(&mut self) {
        self.shared.config.store(Arc::new(self.store.value.clone()));
        self.active_profile = config::profile_for_aliases(&self.store.value, &self.foreground.aliases).to_owned();
        {
            let mut snapshot = self.shared.snapshot.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            snapshot.config = self.store.value.clone();
            snapshot.revision = self.store.revision;
            snapshot.notice.clone_from(&self.store.notice);
        }
        self.publish_policy();
    }

    fn apply(&mut self, revision: u64, value: Value) -> io::Result<()> {
        let executable = &self.executable;
        let headless = self.shared.headless;
        apply_settings(&mut self.store, revision, value, |enabled| {
            if headless { return Err(io::Error::new(io::ErrorKind::Unsupported, "Login integration is disabled in headless verification mode")); }
            native::set_start_at_login(enabled, executable)
        })?;
        self.publish_configuration();
        Ok(())
    }

    fn show_settings(&mut self) -> io::Result<()> {
        if self.shared.headless { return Err(io::Error::new(io::ErrorKind::Unsupported, "The settings window is disabled in headless verification mode")); }
        if let Some(child) = self.ui.as_mut() {
            if child.try_wait()?.is_none() {
                native::post(native::UiEvent::Focus(child.id()));
                return Ok(());
            }
            self.ui = None;
        }
        let name = if cfg!(windows) { "superlight-ui.exe" } else { "superlight-ui" };
        let path = self.executable.with_file_name(name);
        if !path.is_file() { return Err(io::Error::new(io::ErrorKind::NotFound, "The superlight-ui executable is missing. Install the complete application bundle.")); }
        let child = ProcessCommand::new(path).arg("--service-running").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()?;
        self.ui = Some(child);
        Ok(())
    }

    fn request(&mut self, request: Request) -> Response {
        let result = match request {
            Request::Get => Ok(()),
            Request::Apply { expected_revision, config } => self.apply(expected_revision, config),
            Request::SetPaused { value } => {
                self.paused = value;
                if value { self.shared.release_all(); }
                self.publish_policy();
                Ok(())
            }
            Request::Reconnect => { self.shared.request_reconnect(); Ok(()) }
            Request::RefreshHardware => { self.shared.refresh.store(true, Ordering::Release); self.shared.wake_hid(); Ok(()) }
            Request::RequestPermissions => {
                if self.shared.headless { Err(io::Error::new(io::ErrorKind::Unsupported, "Native permissions are disabled in headless verification mode")) }
                else { native::post(native::UiEvent::Permissions); Ok(()) }
            }
            Request::ShowSettings => self.show_settings(),
            Request::Quit => { self.shared.stop(); Ok(()) }
        };
        match result { Ok(()) => Response::success(self.shared.status()), Err(error) => Response::failure(error) }
    }

    fn handle(&mut self, command: Command) {
        match command {
            Command::Rpc { request, reply } => { let _ = reply.try_send(self.request(request)); }
            Command::Request(request) => {
                let response = self.request(request);
                if let Some(error) = response.error { self.record_error(error); }
            }
            Command::Device(device) => {
                let mut snapshot = self.shared.snapshot.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                snapshot.device = device;
            }
            Command::HardwarePending(pending) => {
                self.shared.snapshot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).hardware_pending = pending;
            }
            Command::Error(error) => self.record_error(error),
            Command::Native { ready, permissions } => {
                if !ready { self.shared.release_all(); }
                self.shared.native_ready.store(ready, Ordering::Release);
                self.shared.generation.fetch_add(1, Ordering::AcqRel);
                self.shared.snapshot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).permissions = permissions;
                self.shared.wake_hid();
            }
            Command::ForegroundChanged => self.refresh_foreground(),
            Command::Action(action) => {
                let device = self.shared.snapshot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).device.clone();
                if let Some(device) = device {
                    let supported = if action == Action::CycleDpi { device.supports_dpi } else { device.supports_smart_shift };
                    if !supported { self.record_error("The connected mouse does not support that device action".into()); return; }
                    match device_action(&self.store.value, action, device.dpi_min, device.dpi_max) {
                        Ok(value) => { if let Err(error) = self.apply(self.store.revision, value) { self.record_error(error.to_string()); } }
                        Err(error) => self.record_error(error),
                    }
                }
            }
        }
    }
}

impl Drop for Controller {
    fn drop(&mut self) {
        if let Some(mut child) = self.ui.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejected_edits_never_change_login_registration() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = Store::open(Paths::in_dir(directory.path())).unwrap();
        let mut value = store.value.clone();
        value["settings"]["start_at_login"] = json!(true);
        assert!(apply_settings(&mut store, 0, value.clone(), |_| panic!("Login modified before revision validation")).is_err());
        value["profiles"]["default"]["mappings"]["middle"] = json!("custom:not-a-key");
        assert!(apply_settings(&mut store, 1, value, |_| panic!("Login modified before action validation")).is_err());
    }

    #[test]
    fn failed_settings_write_restores_the_previous_login_registration() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = Store::open(Paths::in_dir(directory.path())).unwrap();
        store.paths.config = directory.path().join("missing").join("config.json");
        let mut value = store.value.clone();
        value["settings"]["start_at_login"] = json!(true);
        let mut calls = Vec::new();
        assert!(apply_settings(&mut store, 1, value, |enabled| { calls.push(enabled); Ok(()) }).is_err());
        assert_eq!(calls, [true, false]);
        assert_eq!(store.revision, 1);
        assert!(!policy::boolean(&store.value, "start_at_login", false));
    }

    #[test]
    fn login_failure_never_publishes_or_persists_the_new_configuration() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = Store::open(Paths::in_dir(directory.path())).unwrap();
        let original = std::fs::read(&store.paths.config).unwrap();
        let mut value = store.value.clone();
        value["settings"]["start_at_login"] = json!(true);
        assert!(apply_settings(&mut store, 1, value, |_| Err(io::Error::other("denied"))).is_err());
        assert_eq!(std::fs::read(&store.paths.config).unwrap(), original);
        assert_eq!(store.revision, 1);
    }

    #[test]
    fn default_dpi_cycle_matches_v36_and_clamps_model_limits() {
        let value = config::defaults();
        let first = device_action(&value, Action::CycleDpi, 200, 8000).unwrap();
        assert_eq!(first["settings"]["dpi"], 800);
        let next = device_action(&first, Action::CycleDpi, 200, 8000).unwrap();
        assert_eq!(next["settings"]["dpi"], 1200);
        let mut custom = value;
        custom["settings"]["dpi_presets"] = json!([0, 9000, 10000]);
        let low = device_action(&custom, Action::CycleDpi, 200, 4000).unwrap();
        assert_eq!(low["settings"]["dpi"], 200);
        let high = device_action(&low, Action::CycleDpi, 200, 4000).unwrap();
        assert_eq!(high["settings"]["dpi"], 4000);
    }

    #[test]
    fn smart_shift_toggle_preserves_the_saved_fallback_mode() {
        let mut value = config::defaults();
        value["settings"]["smart_shift_mode"] = json!("freespin");
        let toggled = device_action(&value, Action::ToggleSmartShift, 200, 8000).unwrap();
        assert_eq!(toggled["settings"]["smart_shift_mode"], "freespin");
        assert_eq!(toggled["settings"]["smart_shift_enabled"], true);
        let fixed = device_action(&toggled, Action::SwitchScrollMode, 200, 8000).unwrap();
        assert_eq!(fixed["settings"]["smart_shift_mode"], "ratchet");
        assert_eq!(fixed["settings"]["smart_shift_enabled"], false);
    }
}
''')

write("crates/superlight-service/src/main.rs", r'''#![cfg_attr(windows, windows_subsystem = "windows")]

use std::{io, path::PathBuf};
use superlight_ipc::{Paths, Request, Response, call, store::read_limited};
use superlight_service::{Options, native};

fn request(request: Request) -> io::Result<Response> {
    let response = call(&Paths::discover()?, &request)?;
    if response.ok { Ok(response) } else { Err(io::Error::other(response.error.unwrap_or_else(|| "Operation failed".into()))) }
}

fn execute() -> io::Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let strings: Vec<_> = args.iter().map(|arg| arg.to_string_lossy()).collect();
    if strings.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("SuperLight\n\nRun without arguments to start the service.\n--background  Start without opening settings\n--headless    Verify control and configuration without native input or HID\n--settings    Open the settings window\n--status      Print the current state as JSON\n--apply FILE  Validate and save a complete configuration\n--pause       Pause remapping and release captured input\n--resume      Resume remapping\n--reconnect   Reopen the Logitech connection\n--refresh     Read hardware settings\n--permissions Request native input permissions\n--quit        Stop the service\n--version     Print the version");
        return Ok(());
    }
    if strings.iter().any(|arg| arg == "--version" || arg == "-V") {
        println!("SuperLight {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if let Some(index) = strings.iter().position(|arg| arg == "--apply") {
        if args.len() != 2 || index != 0 { return Err(io::Error::other("Usage: superlight --apply FILE")); }
        let bytes = read_limited(&PathBuf::from(&args[1]), superlight_core::CONFIG_LIMIT)?;
        let config = superlight_core::config::parse(&bytes).map_err(io::Error::other)?;
        let snapshot = request(Request::Get)?.snapshot.ok_or_else(|| io::Error::other("Missing service state"))?;
        let response = request(Request::Apply { expected_revision: snapshot.revision, config })?;
        println!("{}", serde_json::to_string(&response).map_err(io::Error::other)?);
        return Ok(());
    }
    if let Some(first) = strings.first() {
        let command = match first.as_ref() {
            "--status" => Some(Request::Get),
            "--pause" => Some(Request::SetPaused { value: true }),
            "--resume" => Some(Request::SetPaused { value: false }),
            "--reconnect" => Some(Request::Reconnect),
            "--refresh" => Some(Request::RefreshHardware),
            "--permissions" => Some(Request::RequestPermissions),
            "--quit" => Some(Request::Quit),
            _ => None,
        };
        if let Some(command) = command {
            if args.len() != 1 { return Err(io::Error::other("Unexpected command arguments")); }
            let response = request(command)?;
            println!("{}", serde_json::to_string(&response).map_err(io::Error::other)?);
            return Ok(());
        }
    }
    let mut options = Options::default();
    for arg in &strings {
        match arg.as_ref() {
            "--background" => options.background = true,
            "--headless" => options.headless = true,
            "--settings" => options.force_ui = true,
            _ => return Err(io::Error::other(format!("Unknown option: {arg}. Use --help."))),
        }
    }
    let paths = Paths::discover()?;
    if let Ok(response) = call(&paths, &Request::Get) && response.ok {
        if !options.background && !options.headless { request(Request::ShowSettings)?; }
        return Ok(());
    }
    superlight_service::run(options)
}

fn main() {
    native::attach_console();
    if let Err(error) = execute() {
        eprintln!("SuperLight: {error}");
        if std::env::args_os().len() == 1 { native::error_dialog(&error.to_string()); }
        std::process::exit(1);
    }
}
''')

write("crates/superlight-service/src/native/mod.rs", r'''use crate::shared::Shared;
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
''')

path = Path("crates/superlight-service/src/hook.rs")
text = path.read_text().replace("let (shared, _, mut hook) = setup();", "let (shared, _receiver, mut hook) = setup();")
path.write_text(text)
