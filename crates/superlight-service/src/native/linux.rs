use super::{UiEvent, linux_desktop, linux_frame::{self, Frame}, login};
use crate::{hook::Hook, shared::{Command, Shared}};
use evdev::{AttributeSet, Device, EventType, InputEvent, KeyCode, RelativeAxisCode, SynchronizationCode, uinput::VirtualDevice};
use std::{io, os::fd::{AsRawFd, FromRawFd, OwnedFd}, path::{Path, PathBuf}, sync::{Arc, Mutex, OnceLock, atomic::{AtomicU32, Ordering}}, time::{Duration, Instant}};
use superlight_core::{actions::{Chord, Platform}, devices, hidpp};
use superlight_ipc::{AppInfo, DeviceStatus, Permissions, store::atomic_write};

static SHARED: OnceLock<Arc<Shared>> = OnceLock::new();
static OUTPUT: Mutex<Option<VirtualDevice>> = Mutex::new(None);
static WAKE: OnceLock<OwnedFd> = OnceLock::new();
static EVENTS: AtomicU32 = AtomicU32::new(0);
const RETRY: Duration = Duration::from_secs(3);

pub fn attach_console() {}
pub fn error_dialog(message: &str) { eprintln!("SuperLight: {message}"); }
pub fn foreground() -> AppInfo { linux_desktop::foreground() }

fn emit(events: &[InputEvent]) -> io::Result<()> {
    let mut output = OUTPUT.lock().map_err(|_| io::Error::other("The virtual input device is unavailable"))?;
    output.as_mut().ok_or_else(|| io::Error::new(io::ErrorKind::PermissionDenied, "Allow access to /dev/uinput before enabling remapping"))?.emit(events)
}

fn mouse_code(button: u8) -> io::Result<KeyCode> {
    [KeyCode::BTN_LEFT, KeyCode::BTN_RIGHT, KeyCode::BTN_MIDDLE, KeyCode::BTN_SIDE, KeyCode::BTN_EXTRA].get(usize::from(button)).copied().ok_or_else(|| io::Error::other("Invalid mouse button"))
}

pub fn mouse(button: u8, down: bool) -> io::Result<()> {
    emit(&[InputEvent::new(EventType::KEY.0, mouse_code(button)?.0, i32::from(down))])
}

pub fn chord(chord: Chord) -> io::Result<()> {
    let keys = chord.codes.get(..usize::from(chord.len)).filter(|keys| !keys.is_empty()).ok_or_else(|| io::Error::other("Invalid shortcut"))?;
    let mut events: [InputEvent; 16] = std::array::from_fn(|_| InputEvent::new(0, 0, 0));
    for (index, &key) in keys.iter().enumerate() {
        events[index] = InputEvent::new(EventType::KEY.0, key, 1);
        events[2 * keys.len() - index - 1] = InputEvent::new(EventType::KEY.0, key, 0);
    }
    if let Err(error) = emit(&events[..keys.len() * 2]) {
        for (index, &key) in keys.iter().rev().enumerate() { events[index] = InputEvent::new(EventType::KEY.0, key, 0); }
        let _ = emit(&events[..keys.len()]);
        return Err(error);
    }
    Ok(())
}

pub fn media(key: u8) -> io::Result<()> {
    let key = [KeyCode::KEY_VOLUMEUP, KeyCode::KEY_VOLUMEDOWN, KeyCode::KEY_MUTE, KeyCode::KEY_PLAYPAUSE, KeyCode::KEY_NEXTSONG, KeyCode::KEY_PREVIOUSSONG].get(usize::from(key)).copied().ok_or_else(|| io::Error::other("Invalid media key"))?;
    chord(Chord { codes: [key.0, 0, 0, 0, 0, 0, 0, 0], len: 1, phased: false })
}

pub fn system(action: u8) -> io::Result<()> {
    let combo = match action { 0 => "super", 1 => "super+w", 2 => "super+d", 3 => "super+a", 4 => "ctrl+super+left", 5 => "ctrl+super+right", _ => return Err(io::Error::other("Invalid system action")) };
    chord(Chord::parse(combo, Platform::Linux).map_err(io::Error::other)?)
}

pub fn scroll(horizontal: bool, delta: i32) -> io::Result<()> {
    emit(&[InputEvent::new(EventType::RELATIVE.0, if horizontal { RelativeAxisCode::REL_HWHEEL.0 } else { RelativeAxisCode::REL_WHEEL.0 }, delta)])
}

fn ensure_output() -> io::Result<()> {
    let mut output = OUTPUT.lock().map_err(|_| io::Error::other("The virtual input device is unavailable"))?;
    if output.is_some() { return Ok(()); }
    let mut keys = AttributeSet::<KeyCode>::new();
    for code in 1..=255 { keys.insert(KeyCode(code)); }
    for button in 0..5 { keys.insert(mouse_code(button)?); }
    let axes: AttributeSet<RelativeAxisCode> = [RelativeAxisCode::REL_X, RelativeAxisCode::REL_Y, RelativeAxisCode::REL_WHEEL, RelativeAxisCode::REL_HWHEEL].into_iter().collect();
    *output = Some(VirtualDevice::builder()?.name("SuperLight configured actions").with_keys(&keys)?.with_relative_axes(&axes)?.build()?);
    Ok(())
}

fn normalized(name: &str) -> String {
    let name = devices::normalize_name(name);
    name.strip_prefix("logitech ").unwrap_or(&name).to_owned()
}

pub fn mouse_matches(status: &DeviceStatus, vendor: u16, product: u16, name: &str) -> bool {
    if vendor != hidpp::VENDOR || name.starts_with("SuperLight") { return false; }
    if normalized(name) == normalized(&status.name) && !name.is_empty() { return true; }
    if let Some(spec) = devices::resolve(product, name) && spec.key == status.model_key { return true; }
    status.receiver_slot == 0xff && (0xb000..=0xbfff).contains(&status.product_id) && product == status.product_id
}

fn choose(status: &DeviceStatus) -> io::Result<Option<Device>> {
    let mut result = None;
    for (_, device) in evdev::enumerate().take(128) {
        let id = device.input_id();
        if !mouse_matches(status, id.vendor(), id.product(), device.name().unwrap_or_default()) { continue; }
        let mouse = device.supported_keys().is_some_and(|keys| keys.contains(KeyCode::BTN_LEFT) && keys.contains(KeyCode::BTN_RIGHT))
            && device.supported_relative_axes().is_some_and(|axes| axes.contains(RelativeAxisCode::REL_X) && axes.contains(RelativeAxisCode::REL_Y));
        if !mouse { continue; }
        if result.is_some() { return Err(io::Error::other("More than one evdev mouse matches the selected HID device. No device was grabbed. Disconnect the duplicate mouse before reconnecting.")); }
        result = Some(device);
    }
    Ok(result)
}

struct MouseInput {
    device: Device,
    mirror: VirtualDevice,
    hook: Hook,
    frame: Frame,
    filtered: Frame,
    forwarded: [bool; 768],
    identity: String,
}

fn identity(status: &DeviceStatus) -> String { format!("{:04x}/{}/{}", status.product_id, status.receiver_slot, status.name) }

impl MouseInput {
    fn open(mut device: Device, status: &DeviceStatus, shared: &Arc<Shared>) -> io::Result<Option<Self>> {
        if device.get_key_state()?.iter().next().is_some() { return Ok(None); }
        let keys = device.supported_keys().ok_or_else(|| io::Error::other("The selected mouse has no button capabilities"))?;
        let axes = device.supported_relative_axes().ok_or_else(|| io::Error::other("The selected mouse has no relative axes"))?;
        let mirror = VirtualDevice::builder()?.name("SuperLight forwarded mouse").with_keys(keys)?.with_relative_axes(axes)?.with_properties(device.properties())?.build()?;
        std::thread::sleep(Duration::from_millis(250));
        if shared.stopping() || device.get_key_state()?.iter().next().is_some() { return Ok(None); }
        device.set_nonblocking(true)?;
        device.grab()?;
        Ok(Some(Self { device, mirror, hook: Hook::new(Arc::clone(shared)), frame: Frame::default(), filtered: Frame::default(), forwarded: [false; 768], identity: identity(status) }))
    }

    fn read(&mut self, shared: &Shared) -> io::Result<()> {
        let Self { device, mirror, hook, frame, filtered, forwarded, .. } = self;
        match device.fetch_events() {
            Ok(events) => {
                for event in events {
                    if event.event_type() == EventType::SYNCHRONIZATION {
                        if event.code() == SynchronizationCode::SYN_DROPPED.0 { return Err(io::Error::other("The mouse event stream lost synchronization. Native input is being restored.")); }
                        if event.code() == SynchronizationCode::SYN_REPORT.0 {
                            linux_frame::filter(frame, hook, shared, filtered)?;
                            if !filtered.events().is_empty() {
                                mirror.emit(filtered.events())?;
                                for event in filtered.events() {
                                    if event.event_type() == EventType::KEY && let Some(held) = forwarded.get_mut(usize::from(event.code())) { *held = event.value() != 0; }
                                }
                            }
                            frame.clear();
                        }
                    } else { frame.push(event)?; }
                }
                Ok(())
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(()),
            Err(error) => Err(error),
        }
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

fn boot_bias() -> Option<f64> {
    let mut boot: libc::timespec = unsafe { std::mem::zeroed() };
    let mut awake: libc::timespec = unsafe { std::mem::zeroed() };
    if unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &mut boot) } != 0 || unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut awake) } != 0 { return None; }
    Some((boot.tv_sec - awake.tv_sec) as f64 + (boot.tv_nsec - awake.tv_nsec) as f64 / 1_000_000_000.0)
}

pub fn post(event: UiEvent) {
    let flags = match event {
        UiEvent::Quit => 1, UiEvent::Permissions => 2, UiEvent::Refresh => 4,
        UiEvent::Focus(pid) => {
            if let Err(error) = linux_desktop::focus(pid) && let Some(shared) = SHARED.get() { shared.report(error); }
            return;
        }
    };
    EVENTS.fetch_or(flags, Ordering::AcqRel);
    if let Some(fd) = WAKE.get() {
        let value = 1u64;
        unsafe { libc::write(fd.as_raw_fd(), (&value as *const u64).cast(), std::mem::size_of::<u64>()); }
    }
}

fn desktop_exec(value: &str) -> String {
    let mut output = String::with_capacity(value.len() + 2);
    output.push('"');
    for character in value.chars() {
        match character {
            '%' => output.push_str("%%"),
            '\\' => output.push_str("\\\\\\\\"),
            '"' | '`' | '$' => { output.push_str("\\\\"); output.push(character); }
            _ => output.push(character),
        }
    }
    output.push('"');
    output
}

pub fn set_start_at_login(enabled: bool, executable: &Path) -> io::Result<()> {
    let executable = login::checked_executable(executable)?;
    let root = std::env::var_os("XDG_CONFIG_HOME").filter(|value| !value.is_empty()).map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config"))).ok_or_else(|| io::Error::other("The user configuration directory is unavailable"))?;
    let directory = root.join("autostart");
    let path = directory.join("io.github.vincenthopf.SuperLight.desktop");
    if enabled {
        std::fs::create_dir_all(&directory)?;
        let content = format!("[Desktop Entry]\nType=Application\nName=SuperLight\nExec={} --background\nTerminal=false\nX-GNOME-Autostart-enabled=true\n", desktop_exec(executable));
        atomic_write(&path, content.as_bytes())
    } else {
        match std::fs::remove_file(path) { Ok(()) => Ok(()), Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()), Err(error) => Err(error) }
    }
}

fn publish(shared: &Shared, ready: bool, inject: bool, description: &str, previous: &mut Option<(bool, Permissions)>) {
    if !ready && shared.native_ready.swap(false, Ordering::AcqRel) { shared.release_all(); }
    if previous.as_ref().is_some_and(|(old_ready, permissions)| *old_ready == ready && permissions.inject == inject && permissions.description == description) { return; }
    let permissions = Permissions { listen: ready, inject, description: description.into() };
    if shared.command(Command::Native { ready, permissions: permissions.clone() }) { *previous = Some((ready, permissions)); }
}

pub fn run(shared: Arc<Shared>) -> io::Result<()> {
    SHARED.set(Arc::clone(&shared)).map_err(|_| io::Error::other("Native input is already running"))?;
    let descriptor = unsafe { libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK) };
    if descriptor < 0 { return Err(io::Error::last_os_error()); }
    WAKE.set(unsafe { OwnedFd::from_raw_fd(descriptor) }).map_err(|_| io::Error::other("The native event loop is already running"))?;
    let wayland = std::env::var("XDG_SESSION_TYPE").is_ok_and(|value| value.eq_ignore_ascii_case("wayland"));
    let ready_description = if wayland { "The selected Logitech mouse is available through evdev/uinput. Native Wayland applications use the default profile because this desktop does not expose a portable foreground-application API." } else { "The selected Logitech mouse is available through evdev/uinput. Other input devices are unchanged." };
    let mut input: Option<MouseInput> = None;
    let mut retry = Instant::now();
    let mut next_status = Instant::now();
    let mut status: Option<DeviceStatus> = None;
    let mut published = None;
    let mut previous_bias = boot_bias();
    let mut injection_ready = false;
    while !shared.stopping() {
        let events = EVENTS.swap(0, Ordering::AcqRel);
        if events & 1 != 0 { break; }
        if events & 2 != 0 {
            shared.report("Linux remapping needs access to the selected Logitech /dev/input/event device, its /dev/hidraw device and /dev/uinput. Install the supplied udev rules, reconnect the mouse and sign in again. Do not run SuperLight as root.");
            retry = Instant::now();
        }
        if events & 4 != 0 || Instant::now() >= next_status {
            next_status = Instant::now() + Duration::from_secs(2);
            status = shared.snapshot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).device.clone();
            if let (Some(before), Some(now)) = (previous_bias, boot_bias()) {
                if now - before > 0.1 { shared.request_reconnect(); input = None; }
                previous_bias = Some(now);
            }
            if input.as_ref().is_some_and(|input| status.as_ref().is_none_or(|status| input.identity != identity(status))) {
                input = None;
                shared.release_all();
            }
        }
        if (!shared.device_connected.load(Ordering::Acquire) || shared.suspended.load(Ordering::Acquire)) && input.take_if(|_| true).is_some() {
            shared.release_all();
        }
        if input.is_none() && shared.native_ready.load(Ordering::Acquire) {
            publish(&shared, false, injection_ready, "Waiting for the verified Logitech mouse connection", &mut published);
        }
        if input.is_none() && Instant::now() >= retry && !shared.suspended.load(Ordering::Acquire) {
            retry = Instant::now() + RETRY;
            injection_ready = match ensure_output() {
                Ok(()) => true,
                Err(error) => { publish(&shared, false, false, &format!("Cannot open /dev/uinput: {error}. Install the supplied user-access udev rule before enabling remapping."), &mut published); false }
            };
            if injection_ready && shared.device_connected.load(Ordering::Acquire) && let Some(status) = &status {
                match choose(status).and_then(|device| match device { Some(device) => MouseInput::open(device, status, &shared), None => Ok(None) }) {
                    Ok(Some(device)) => input = Some(device),
                    Ok(None) => publish(&shared, false, true, "No accessible evdev mouse matches the selected Logitech HID device, or a button is still held. Release all buttons and check the supplied udev rules.", &mut published),
                    Err(error) => publish(&shared, false, true, &error.to_string(), &mut published),
                }
            } else if injection_ready {
                publish(&shared, false, true, "Waiting for a verified Logitech HID++ mouse. Other input devices have not been grabbed.", &mut published);
            }
        }
        if input.is_some() { publish(&shared, true, injection_ready, ready_description, &mut published); }
        let mut descriptors = [
            libc::pollfd { fd: descriptor, events: libc::POLLIN, revents: 0 },
            libc::pollfd { fd: input.as_ref().map_or(-1, |input| input.device.as_raw_fd()), events: libc::POLLIN, revents: 0 },
        ];
        let timeout = if input.is_some() { 2000 } else { retry.saturating_duration_since(Instant::now()).as_millis().clamp(1, 3000) as i32 };
        let result = unsafe { libc::poll(descriptors.as_mut_ptr(), descriptors.len() as libc::nfds_t, timeout) };
        if result < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted { continue; }
            return Err(error);
        }
        if descriptors[0].revents & libc::POLLIN != 0 {
            let mut value = 0u64;
            unsafe { libc::read(descriptor, (&mut value as *mut u64).cast(), std::mem::size_of::<u64>()); }
        }
        let failure = if descriptors[1].revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
            Some(io::Error::other("The Logitech input device disconnected"))
        } else if descriptors[1].revents & libc::POLLIN != 0 {
            input.as_mut().and_then(|input| input.read(&shared).err())
        } else { None };
        if let Some(error) = failure {
            input = None;
            shared.release_all();
            publish(&shared, false, injection_ready, &format!("Native input was restored: {error}"), &mut published);
            retry = Instant::now() + RETRY;
        }
    }
    shared.native_ready.store(false, Ordering::Release);
    shared.release_all();
    drop(input);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receiver_product_ids_do_not_authorize_unrelated_mouse_nodes() {
        let status = DeviceStatus { name: "MX Master 3S".into(), model_key: "mx_master_3s".into(), product_id: 0xc548, receiver_slot: 2, ..DeviceStatus::default() };
        assert!(mouse_matches(&status, 0x046d, 0xb034, "Logitech MX Master 3S"));
        assert!(!mouse_matches(&status, 0x046d, 0xc548, "Another receiver device"));
        assert!(!mouse_matches(&status, 0x1234, 0xb034, "MX Master 3S"));
        assert!(!mouse_matches(&status, 0x046d, 0xb020, "MX Vertical"));
    }

    #[test]
    fn desktop_entry_paths_are_quoted_without_field_expansion() {
        assert_eq!(desktop_exec("/home/me/My App/superlight"), "\"/home/me/My App/superlight\"");
        assert_eq!(desktop_exec("/home/me/100%/superlight"), "\"/home/me/100%%/superlight\"");
        assert!(desktop_exec("a\"b").contains("\\\\\""));
        assert!(login::checked_executable(Path::new("bad\nExec=other")).is_err());
    }
}
