use super::{UiEvent, login};
use crate::{hook::Hook, shared::{Command, Shared}};
use evdev::{AttributeSet, Device, EventSummary, EventType, InputEvent, KeyCode, RelativeAxisCode, uinput::VirtualDevice};
use std::{io, os::fd::AsRawFd, path::Path, sync::{Arc, Mutex, atomic::{AtomicU32, Ordering}}, time::{Duration, Instant}};
use superlight_core::actions::{Chord, Platform};
use superlight_ipc::{AppInfo, Permissions, store::atomic_write};

static OUTPUT: Mutex<Option<VirtualDevice>> = Mutex::new(None);
static EVENTS: AtomicU32 = AtomicU32::new(0);

pub fn attach_console() {}
pub fn error_dialog(message: &str) { eprintln!("SuperLight: {message}"); }
pub fn foreground() -> AppInfo { AppInfo::default() }

fn create_output() -> io::Result<VirtualDevice> {
    let mut keys = AttributeSet::<KeyCode>::new();
    for code in 1..=255 { keys.insert(KeyCode(code)); }
    for code in 272..=276 { keys.insert(KeyCode(code)); }
    let axes: AttributeSet<RelativeAxisCode> = [RelativeAxisCode::REL_X, RelativeAxisCode::REL_Y, RelativeAxisCode::REL_WHEEL, RelativeAxisCode::REL_HWHEEL].into_iter().collect();
    VirtualDevice::builder()?.name("SuperLight configured actions").with_keys(&keys)?.with_relative_axes(&axes)?.build()
}

fn emit(events: &[InputEvent]) -> io::Result<()> {
    let mut guard = OUTPUT.lock().map_err(|_| io::Error::other("The output device is unavailable"))?;
    guard.as_mut().ok_or_else(|| io::Error::new(io::ErrorKind::PermissionDenied, "Allow access to /dev/uinput"))?.emit(events)
}

pub fn mouse(button: u8, down: bool) -> io::Result<()> {
    let code = [272, 273, 274, 275, 276].get(usize::from(button)).copied().ok_or_else(|| io::Error::other("Invalid mouse button"))?;
    emit(&[InputEvent::new(EventType::KEY.0, code, i32::from(down))])
}

pub fn chord(chord: Chord) -> io::Result<()> {
    let mut down = [InputEvent::new(0, 0, 0); 8];
    let mut up = [InputEvent::new(0, 0, 0); 8];
    for (index, &key) in chord.keys().iter().enumerate() {
        down[index] = InputEvent::new(EventType::KEY.0, key, 1);
        up[chord.keys().len() - index - 1] = InputEvent::new(EventType::KEY.0, key, 0);
    }
    let count = chord.keys().len();
    if let Err(error) = emit(&down[..count]) { let _ = emit(&up[..count]); return Err(error); }
    std::thread::sleep(Duration::from_millis(50));
    emit(&up[..count])
}

pub fn scroll(horizontal: bool, delta: i32) -> io::Result<()> {
    emit(&[InputEvent::new(EventType::RELATIVE.0, if horizontal { RelativeAxisCode::REL_HWHEEL.0 } else { RelativeAxisCode::REL_WHEEL.0 }, delta)])
}

pub fn media(key: u8) -> io::Result<()> {
    let code = [115, 114, 113, 164, 163, 165].get(usize::from(key)).copied().ok_or_else(|| io::Error::other("Invalid media action"))?;
    chord(Chord { codes: [code, 0, 0, 0, 0, 0, 0, 0], len: 1, phased: false })
}

pub fn system(action: u8) -> io::Result<()> {
    let keys = match action {
        0 => "super", 1 => "super+w", 2 => "super+d", 3 => "super+a", 4 => "ctrl+super+left", 5 => "ctrl+super+right",
        _ => return Err(io::Error::other("Invalid desktop action")),
    };
    chord(Chord::parse(keys, Platform::Linux).map_err(io::Error::other)?)
}

pub fn set_start_at_login(enabled: bool, executable: &Path) -> io::Result<()> {
    let executable = login::checked_executable(executable)?;
    let directory = std::env::var_os("XDG_CONFIG_HOME").map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| Path::new(&home).join(".config")))
        .ok_or_else(|| io::Error::other("A user configuration directory is required"))?.join("autostart");
    let path = directory.join("io.github.vincenthopf.SuperLight.desktop");
    if enabled {
        std::fs::create_dir_all(directory)?;
        let quoted = executable.replace('\\', "\\\\").replace('"', "\\\"").replace('`', "\\`").replace('$', "\\$").replace('%', "%%");
        let content = format!("[Desktop Entry]\nType=Application\nName=SuperLight\nExec=\"{quoted}\" --background\nTerminal=false\nX-GNOME-Autostart-enabled=true\n");
        atomic_write(&path, content.as_bytes())?;
    } else {
        match std::fs::remove_file(path) { Ok(()) => {}, Err(error) if error.kind() == io::ErrorKind::NotFound => {}, Err(error) => return Err(error) }
    }
    Ok(())
}

pub fn post(event: UiEvent) {
    let bit = match event { UiEvent::Quit => 1, UiEvent::Permissions => 2, UiEvent::Refresh => 4, UiEvent::Focus(_) => 8 };
    EVENTS.fetch_or(bit, Ordering::AcqRel);
}

pub fn is_logitech_mouse(device: &Device) -> bool {
    device.input_id().vendor() == 0x046d
        && device.supported_keys().is_some_and(|keys| keys.contains(KeyCode::BTN_LEFT) && keys.contains(KeyCode::BTN_RIGHT))
        && device.supported_relative_axes().is_some_and(|axes| axes.contains(RelativeAxisCode::REL_X) && axes.contains(RelativeAxisCode::REL_Y))
        && !device.name().unwrap_or_default().starts_with("SuperLight")
}

struct MouseInput {
    device: Device,
    mirror: VirtualDevice,
    hook: Hook,
    forwarded: [bool; 768],
    hires: bool,
}

impl MouseInput {
    fn open(shared: Arc<Shared>) -> io::Result<Self> {
        let status = shared.status().device;
        let mut candidates: Vec<_> = evdev::enumerate().filter(|(_, device)| is_logitech_mouse(device)).collect();
        candidates.sort_by_key(|(path, device)| (
            u8::from(status.as_ref().is_none_or(|status| status.product_id != device.input_id().product())),
            path.clone(),
        ));
        for (_, mut device) in candidates.into_iter().take(16) {
            if device.get_key_state()?.iter().any(|key| key.0 >= 272) { continue; }
            let mut builder = VirtualDevice::builder()?.name("SuperLight forwarded mouse");
            if let Some(keys) = device.supported_keys() { builder = builder.with_keys(keys)?; }
            if let Some(axes) = device.supported_relative_axes() { builder = builder.with_relative_axes(axes)?; }
            if let Some(misc) = device.misc_properties() { builder = builder.with_msc(misc)?; }
            let mirror = builder.build()?;
            device.set_nonblocking(true)?;
            device.grab()?;
            let hires = device.supported_relative_axes().is_some_and(|axes| axes.contains(RelativeAxisCode::REL_HWHEEL_HI_RES));
            return Ok(Self { device, mirror, hook: Hook::new(shared), forwarded: [false; 768], hires });
        }
        Err(io::Error::new(io::ErrorKind::NotFound, "No accessible, idle Logitech mouse event device. Grant access to the Logitech /dev/input/event* devices and /dev/uinput."))
    }

    fn read(&mut self, shared: &Shared) -> io::Result<()> {
        let Self { device, mirror, hook, forwarded, hires } = self;
        let mut batch = [InputEvent::new(0, 0, 0); 64];
        let mut count = 0;
        let events = match device.fetch_events() { Ok(events) => events, Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(()), Err(error) => return Err(error) };
        for event in events {
            let mut output = event;
            let blocked = match event.destructure() {
                EventSummary::Synchronization(_, _, _) => {
                    if count != 0 { mirror.emit(&batch[..count])?; count = 0; }
                    continue;
                }
                EventSummary::Key(_, key, value) => {
                    let source = match key { KeyCode::BTN_MIDDLE => Some(0), KeyCode::BTN_SIDE | KeyCode::BTN_BACK => Some(2), KeyCode::BTN_EXTRA | KeyCode::BTN_FORWARD => Some(3), _ => None };
                    let blocked = source.is_some_and(|source| hook.button(source, value != 0));
                    if !blocked && let Some(held) = forwarded.get_mut(usize::from(key.0)) { *held = value != 0; }
                    blocked
                }
                EventSummary::RelativeAxis(_, axis, value) => {
                    if axis == RelativeAxisCode::REL_X { hook.movement(f64::from(value), 0.0) }
                    else if axis == RelativeAxisCode::REL_Y { hook.movement(0.0, f64::from(value)) }
                    else {
                        let horizontal = matches!(axis, RelativeAxisCode::REL_HWHEEL | RelativeAxisCode::REL_HWHEEL_HI_RES);
                        let vertical = matches!(axis, RelativeAxisCode::REL_WHEEL | RelativeAxisCode::REL_WHEEL_HI_RES);
                        let policy = shared.policy.load();
                        let source = if value < 0 { 4 } else { 5 };
                        let blocked = horizontal && value != 0 && if *hires && axis == RelativeAxisCode::REL_HWHEEL {
                            shared.allowed() && policy.action(usize::from(source)) != superlight_core::actions::Action::None
                        } else { hook.wheel(source, f64::from(value) / if axis == RelativeAxisCode::REL_HWHEEL_HI_RES { 120.0 } else { 1.0 }) };
                        if shared.allowed() && ((horizontal && policy.invert_horizontal) || (vertical && policy.invert_vertical)) {
                            output = InputEvent::new(EventType::RELATIVE.0, axis.0, value.saturating_neg());
                        }
                        blocked
                    }
                }
                _ => false,
            };
            if !blocked {
                batch[count] = output;
                count += 1;
                if count == batch.len() { mirror.emit(&batch)?; count = 0; }
            }
        }
        if count != 0 { mirror.emit(&batch[..count])?; }
        Ok(())
    }
}

impl Drop for MouseInput {
    fn drop(&mut self) {
        for (code, held) in self.forwarded.iter().enumerate() {
            if *held { let _ = self.mirror.emit(&[InputEvent::new(EventType::KEY.0, code as u16, 0)]); }
        }
        let _ = self.device.ungrab();
    }
}

fn publish(shared: &Shared, ready: bool, detail: &str) {
    if !ready { shared.release_all(); }
    shared.native_ready.store(ready, Ordering::Release);
    shared.command(Command::Native { ready, permissions: Permissions { listen: ready, inject: ready, description: detail.into() } });
}

pub fn run(shared: Arc<Shared>) -> io::Result<()> {
    let mut input: Option<MouseInput> = None;
    let mut retry = Instant::now();
    let mut last_error = String::new();
    while !shared.stopping() {
        let events = EVENTS.swap(0, Ordering::AcqRel);
        if events & 1 != 0 { break; }
        if events & 2 != 0 { retry = Instant::now(); }
        let available = shared.device_connected.load(Ordering::Acquire) && !shared.suspended.load(Ordering::Acquire);
        if !available && input.is_some() {
            publish(&shared, false, "Waiting for the Logitech connection");
            input = None;
        }
        if available && input.is_none() && Instant::now() >= retry {
            let result = (|| {
                let mut output = OUTPUT.lock().map_err(|_| io::Error::other("The output device is unavailable"))?;
                if output.is_none() { *output = Some(create_output()?); }
                drop(output);
                MouseInput::open(Arc::clone(&shared))
            })();
            match result {
                Ok(device) => {
                    input = Some(device);
                    last_error.clear();
                    publish(&shared, true, "Logitech evdev and uinput are available. Application-specific profile detection is not available in this Linux backend.");
                }
                Err(error) => {
                    let message = error.to_string();
                    if message != last_error { publish(&shared, false, &message); shared.report(&message); last_error = message; }
                    retry = Instant::now() + Duration::from_secs(3);
                }
            }
        }
        if let Some(device) = input.as_mut() {
            let mut descriptor = libc::pollfd { fd: device.device.as_raw_fd(), events: libc::POLLIN, revents: 0 };
            let result = unsafe { libc::poll(&mut descriptor, 1, 250) };
            if result < 0 {
                let error = io::Error::last_os_error();
                if error.kind() != io::ErrorKind::Interrupted { return Err(error); }
            } else if descriptor.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
                publish(&shared, false, "The Logitech input device disconnected");
                input = None;
                retry = Instant::now();
            } else if result > 0 && let Err(error) = device.read(&shared) {
                shared.report(error);
                publish(&shared, false, "The Logitech input stream failed. Native mouse handling has been restored.");
                input = None;
                retry = Instant::now() + Duration::from_secs(3);
            }
        } else { shared.wait(Duration::from_millis(250)); }
    }
    publish(&shared, false, "SuperLight is stopping");
    drop(input);
    Ok(())
}
