from pathlib import Path


def write(path, content):
    file = Path(path)
    file.parent.mkdir(parents=True, exist_ok=True)
    file.write_text(content)


write("crates/superlight-service/src/native/macos.rs", r'''#![allow(unsafe_op_in_unsafe_fn)]

use super::{UiEvent, login, macos_ffi::*};
use crate::{hook::Hook, shared::{Command, Shared}};
use std::{cell::{Cell, RefCell}, ffi::{CStr, c_void}, io, path::Path, ptr, sync::{Arc, Mutex, OnceLock, atomic::{AtomicU32, Ordering}}, time::Duration};
use superlight_core::{actions::Chord, input::{INJECTION_MARKER, INVERT_MARKER}};
use superlight_ipc::{AppInfo, Permissions, Request, store::atomic_write};

static SHARED: OnceLock<Arc<Shared>> = OnceLock::new();
static LOOP_SOURCE: Mutex<Option<(usize, usize)>> = Mutex::new(None);
static EVENTS: AtomicU32 = AtomicU32::new(0);
static FOCUS_PID: AtomicU32 = AtomicU32::new(0);

thread_local! {
    static HOOK: RefCell<Option<Hook>> = const { RefCell::new(None) };
    static TAP: Cell<Cf> = const { Cell::new(ptr::null()) };
    static NATIVE: RefCell<Option<Native>> = const { RefCell::new(None) };
}

pub fn attach_console() {}

fn permitted_output() -> io::Result<()> {
    if unsafe { IsSecureEventInputEnabled() } { return Err(io::Error::new(io::ErrorKind::PermissionDenied, "Secure Input is active. Remapping is paused.")); }
    if !unsafe { CGPreflightPostEventAccess() || AXIsProcessTrusted() } {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "Allow SuperLight in Privacy & Security > Accessibility"));
    }
    Ok(())
}

fn mark_and_post(event: Cf) {
    unsafe {
        CGEventSetIntegerValueField(event, 42, INJECTION_MARKER);
        CGEventPost(0, event);
    }
}

pub fn mouse(button: u8, down: bool) -> io::Result<()> {
    if button > 4 { return Err(io::Error::other("Invalid mouse button")); }
    let current = Owned::new(unsafe { CGEventCreate(ptr::null()) })?;
    let point = unsafe { CGEventGetLocation(current.0) };
    let kind = match (button, down) { (0, true) => 1, (0, false) => 2, (1, true) => 3, (1, false) => 4, (_, true) => 25, (_, false) => 26 };
    let event = Owned::new(unsafe { CGEventCreateMouseEvent(ptr::null(), kind, point, u32::from(button)) })?;
    mark_and_post(event.0);
    Ok(())
}

fn modifier(key: u16) -> u64 {
    match key { 55 | 54 => 1 << 20, 56 | 60 => 1 << 17, 58 | 61 => 1 << 19, 59 | 62 => 1 << 18, _ => 0 }
}

pub fn chord(chord: Chord) -> io::Result<()> {
    permitted_output()?;
    let mut down = Vec::with_capacity(chord.keys().len());
    let mut up = Vec::with_capacity(chord.keys().len());
    let flags = chord.keys().iter().fold(0, |flags, key| flags | modifier(*key));
    for &key in chord.keys() {
        let event = Owned::new(unsafe { CGEventCreateKeyboardEvent(ptr::null(), key, 1) })?;
        unsafe { CGEventSetFlags(event.0, flags); }
        down.push(event);
        up.push(Owned::new(unsafe { CGEventCreateKeyboardEvent(ptr::null(), key, 0) })?);
    }
    for event in &down { mark_and_post(event.0); }
    std::thread::sleep(Duration::from_millis(50));
    for event in up.iter().rev() { mark_and_post(event.0); }
    Ok(())
}

pub fn scroll(horizontal: bool, delta: i32) -> io::Result<()> {
    let event = Owned::new(unsafe {
        if horizontal { CGEventCreateScrollWheelEvent(ptr::null(), 0, 2, 0i32, delta) }
        else { CGEventCreateScrollWheelEvent(ptr::null(), 0, 1, delta) }
    })?;
    mark_and_post(event.0);
    Ok(())
}

pub fn media(key: u8) -> io::Result<()> {
    permitted_output()?;
    let key = [0isize, 1, 7, 16, 17, 18].get(usize::from(key)).copied().ok_or_else(|| io::Error::other("Invalid media key"))?;
    let _pool = Pool::new();
    unsafe {
        for state in [0xaisize, 0xb] {
            let event = event(14, (state << 8) as usize, 8, key << 16 | state << 8, -1);
            if event.is_null() { return Err(io::Error::other("Could not create a native media event")); }
            let cg: Cf = msg0(event, c"CGEvent");
            if cg.is_null() { return Err(io::Error::other("Could not convert the native media event")); }
            mark_and_post(cg);
        }
    }
    Ok(())
}

unsafe fn dock_notification(name: &str) -> bool {
    let function = symbol(c"CoreDockSendNotification");
    if function.is_null() { return false; }
    let function: unsafe extern "C" fn(Cf, i32) -> i32 = std::mem::transmute(function);
    string(name).is_ok_and(|name| function(name.0, 0) == 0)
}

unsafe fn symbolic_hotkey(hotkey: u32) -> io::Result<bool> {
    let get = symbol(c"CGSGetSymbolicHotKeyValue");
    let enabled = symbol(c"CGSIsSymbolicHotKeyEnabled");
    let set = symbol(c"CGSSetSymbolicHotKeyEnabled");
    if get.is_null() || enabled.is_null() || set.is_null() { return Ok(false); }
    let get: unsafe extern "C" fn(u32, *mut u16, *mut u16, *mut u32) -> i32 = std::mem::transmute(get);
    let enabled: unsafe extern "C" fn(u32) -> bool = std::mem::transmute(enabled);
    let set: unsafe extern "C" fn(u32, bool) -> i32 = std::mem::transmute(set);
    let (mut equivalent, mut key, mut flags) = (0u16, 0u16, 0u32);
    if get(hotkey, &mut equivalent, &mut key, &mut flags) != 0 || key == u16::MAX { return Ok(false); }
    let down = Owned::new(CGEventCreateKeyboardEvent(ptr::null(), key, 1))?;
    let up = Owned::new(CGEventCreateKeyboardEvent(ptr::null(), key, 0))?;
    let was_enabled = enabled(hotkey);
    if !was_enabled && set(hotkey, true) != 0 { return Ok(false); }
    CGEventSetFlags(down.0, u64::from(flags));
    CGEventSetFlags(up.0, u64::from(flags));
    CGEventSetIntegerValueField(down.0, 42, INJECTION_MARKER);
    CGEventSetIntegerValueField(up.0, 42, INJECTION_MARKER);
    CGEventPost(1, down.0);
    CGEventPost(1, up.0);
    if !was_enabled {
        std::thread::sleep(Duration::from_millis(50));
        if set(hotkey, false) != 0 { return Err(io::Error::other("The desktop shortcut ran, but macOS could not restore its previous disabled state")); }
    }
    Ok(true)
}

pub fn system(action: u8) -> io::Result<()> {
    permitted_output()?;
    let notification = match action {
        0 => Some("com.apple.expose.awake"), 1 => Some("com.apple.expose.front.awake"),
        2 => Some("com.apple.showdesktop.awake"), 3 => Some("com.apple.launchpad.toggle"), _ => None,
    };
    if notification.is_some_and(|name| unsafe { dock_notification(name) }) { return Ok(()); }
    if matches!(action, 4 | 5) && unsafe { symbolic_hotkey(if action == 4 { 79 } else { 81 })? } { return Ok(()); }
    let keys: &[u16] = match action { 0 => &[59, 126], 1 => &[59, 125], 2 => &[103], 3 => &[118], 4 => &[59, 123], 5 => &[59, 124], _ => return Err(io::Error::other("Invalid system action")) };
    let mut value = Chord { codes: [0; 8], len: keys.len() as u8, phased: false };
    value.codes[..keys.len()].copy_from_slice(keys);
    chord(value)
}

pub fn foreground() -> AppInfo {
    let _pool = Pool::new();
    unsafe {
        let workspace: Id = msg0(class(c"NSWorkspace"), c"sharedWorkspace");
        let app: Id = msg0(workspace, c"frontmostApplication");
        if app.is_null() { return AppInfo { input_restricted: true, ..AppInfo::default() }; }
        let pid: i32 = msg0(app, c"processIdentifier");
        let name: Id = msg0(app, c"localizedName");
        let identifier: Id = msg0(app, c"bundleIdentifier");
        let executable: Id = msg0(app, c"executableURL");
        let path: Id = if executable.is_null() { ptr::null_mut() } else { msg0(executable, c"path") };
        let name = text(name.cast());
        let id = text(identifier.cast());
        let path = text(path.cast());
        let mut aliases = Vec::with_capacity(4);
        for alias in [name.clone(), id.clone(), Path::new(&path).file_name().and_then(|value| value.to_str()).unwrap_or_default().into(), path] {
            if !alias.is_empty() && !aliases.contains(&alias) { aliases.push(alias); }
        }
        AppInfo { pid: pid.max(0) as u32, name, id, aliases, input_restricted: IsSecureEventInputEnabled() }
    }
}

pub fn set_start_at_login(enabled: bool, executable: &Path) -> io::Result<()> {
    let executable = login::checked_executable(executable)?;
    let home = std::env::var_os("HOME").ok_or_else(|| io::Error::other("HOME is not configured"))?;
    let directory = Path::new(&home).join("Library/LaunchAgents");
    let path = directory.join("io.github.vincenthopf.SuperLight.plist");
    if enabled {
        std::fs::create_dir_all(&directory)?;
        let plist = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\"><plist version=\"1.0\"><dict><key>Label</key><string>io.github.vincenthopf.SuperLight</string><key>ProgramArguments</key><array><string>{}</string><string>--background</string></array><key>RunAtLoad</key><true/><key>ProcessType</key><string>Interactive</string></dict></plist>", login::xml(executable));
        atomic_write(&path, plist.as_bytes())?;
    } else {
        match std::fs::remove_file(path) { Ok(()) => {}, Err(error) if error.kind() == io::ErrorKind::NotFound => {}, Err(error) => return Err(error) }
    }
    Ok(())
}

pub fn error_dialog(message: &str) {
    let _pool = Pool::new();
    unsafe {
        let alert: Id = msg0(class(c"NSAlert"), c"new");
        if alert.is_null() { return; }
        if let (Ok(title), Ok(detail)) = (string("SuperLight"), string(message)) {
            msg1::<_, ()>(alert, c"setMessageText:", title.0);
            msg1::<_, ()>(alert, c"setInformativeText:", detail.0);
            let _: isize = msg0(alert, c"runModal");
        }
        msg0::<()>(alert, c"release");
    }
}

pub fn post(event: UiEvent) {
    let flag = match event { UiEvent::Quit => 1, UiEvent::Permissions => 2, UiEvent::Refresh => 4, UiEvent::Focus(pid) => { FOCUS_PID.store(pid, Ordering::Release); 8 } };
    EVENTS.fetch_or(flag, Ordering::AcqRel);
    if let Some((source, loop_ref)) = *LOOP_SOURCE.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) {
        unsafe { CFRunLoopSourceSignal(source as Cf); CFRunLoopWakeUp(loop_ref as Cf); }
    }
}

unsafe extern "C" fn perform(_: *mut c_void) {
    let result = std::panic::catch_unwind(|| {
        let events = EVENTS.swap(0, Ordering::AcqRel);
        NATIVE.with(|native| {
            if let Ok(mut native) = native.try_borrow_mut() && let Some(native) = native.as_mut() {
                if events & 2 != 0 { native.request_permissions(); }
                if events & 4 != 0 { native.refresh_permissions(); }
                if events & 8 != 0 {
                    let app: Id = msg1(class(c"NSRunningApplication"), c"runningApplicationWithProcessIdentifier:", FOCUS_PID.load(Ordering::Acquire) as i32);
                    if !app.is_null() { let _: bool = msg1(app, c"activateWithOptions:", 2usize); }
                }
                if events & 1 != 0 {
                    msg1::<_, ()>(native.app, c"stop:", ptr::null_mut::<c_void>());
                    let event = event(15, 0, 0, 0, 0);
                    if !event.is_null() { msg2::<_, _, ()>(native.app, c"postEvent:atStart:", event, true); }
                }
            }
        });
    });
    if result.is_err() && let Some(shared) = SHARED.get() { shared.release_all(); }
}

fn issue(request: Request) { if let Some(shared) = SHARED.get() { shared.command(Command::Request(request)); } }
unsafe extern "C" fn settings(_: Id, _: Sel, _: Id) { issue(Request::ShowSettings); }
unsafe extern "C" fn reconnect(_: Id, _: Sel, _: Id) { issue(Request::Reconnect); }
unsafe extern "C" fn permissions(_: Id, _: Sel, _: Id) { post(UiEvent::Permissions); }
unsafe extern "C" fn quit(_: Id, _: Sel, _: Id) { if let Some(shared) = SHARED.get() { shared.stop(); } }
unsafe extern "C" fn pause(_: Id, _: Sel, _: Id) {
    if let Some(shared) = SHARED.get() { issue(Request::SetPaused { value: !shared.policy.load().paused }); }
}
unsafe extern "C" fn foreground_changed(_: Id, _: Sel, _: Id) {
    if let Some(shared) = SHARED.get() { shared.command(Command::ForegroundChanged); }
}
unsafe extern "C" fn sleeping(_: Id, _: Sel, _: Id) {
    if let Some(shared) = SHARED.get() { shared.suspended.store(true, Ordering::Release); shared.request_reconnect(); }
}
unsafe extern "C" fn awake(_: Id, _: Sel, _: Id) {
    if let Some(shared) = SHARED.get() { shared.suspended.store(false, Ordering::Release); shared.request_reconnect(); }
    post(UiEvent::Refresh);
}

fn button_source(button: i64) -> Option<usize> { match button { 2 => Some(0), 3 => Some(2), 4 => Some(3), _ => None } }

unsafe fn process_event(kind: u32, event: Cf) -> Cf {
    let Some(shared) = SHARED.get() else { return event; };
    if kind == 0xffff_fffe || kind == 0xffff_ffff {
        shared.release_all();
        TAP.with(|tap| { if !tap.get().is_null() && !shared.stopping() { CGEventTapEnable(tap.get(), 1); } });
        return event;
    }
    if event.is_null() { return event; }
    let marker = CGEventGetIntegerValueField(event, 42);
    if marker == INJECTION_MARKER || marker == INVERT_MARKER { return event; }
    HOOK.with(|hook| {
        let Ok(mut hook) = hook.try_borrow_mut() else { return event; };
        let Some(hook) = hook.as_mut() else { return event; };
        match kind {
            25 | 26 => {
                if let Some(source) = button_source(CGEventGetIntegerValueField(event, 3)) && hook.button(source, kind == 25) { return ptr::null(); }
            }
            5 | 6 | 7 | 27 => {
                if hook.movement(CGEventGetIntegerValueField(event, 4) as f64, CGEventGetIntegerValueField(event, 5) as f64) { return ptr::null(); }
                if kind == 27 && let Some(source) = button_source(CGEventGetIntegerValueField(event, 3))
                    && let Some(button) = hook.dragged_button(source) {
                    CGEventSetType(event, match button { 0 => 6, 1 => 7, _ => 27 });
                    CGEventSetIntegerValueField(event, 3, i64::from(button));
                    CGEventSetIntegerValueField(event, 42, INJECTION_MARKER);
                }
            }
            22 if shared.allowed() => {
                let policy = shared.policy.load();
                if policy.ignore_trackpad && CGEventGetIntegerValueField(event, 88) != 0 { return event; }
                let horizontal = CGEventGetDoubleValueField(event, 94);
                let horizontal = if horizontal != 0.0 { horizontal } else { CGEventGetIntegerValueField(event, 12) as f64 };
                if horizontal != 0.0 && hook.wheel(if horizontal > 0.0 { 5 } else { 4 }, horizontal) { return ptr::null(); }
                let mut changed = false;
                for (invert, integer, fixed, point) in [(policy.invert_vertical, 11, 93, 96), (policy.invert_horizontal, 12, 94, 97)] {
                    if invert {
                        CGEventSetIntegerValueField(event, integer, CGEventGetIntegerValueField(event, integer).saturating_neg());
                        CGEventSetDoubleValueField(event, fixed, -CGEventGetDoubleValueField(event, fixed));
                        CGEventSetIntegerValueField(event, point, CGEventGetIntegerValueField(event, point).saturating_neg());
                        changed = true;
                    }
                }
                if changed { CGEventSetIntegerValueField(event, 42, INVERT_MARKER); }
            }
            _ => {}
        }
        event
    })
}

unsafe extern "C" fn tap_callback(_: Cf, kind: u32, event: Cf, _: *mut c_void) -> Cf {
    std::panic::catch_unwind(|| process_event(kind, event)).unwrap_or_else(|_| {
        if let Some(shared) = SHARED.get() { shared.release_all(); }
        event
    })
}

struct Native {
    app: Id,
    delegate: Id,
    status: Id,
    center: Id,
    pause_item: Id,
    tap: Option<Owned>,
    tap_source: Option<Owned>,
    source: Owned,
    last_permissions: Option<Permissions>,
}

impl Native {
    unsafe fn new() -> io::Result<Self> {
        let app: Id = msg0(class(c"NSApplication"), c"sharedApplication");
        let _: bool = msg1(app, c"setActivationPolicy:", 1isize);
        let delegate_class = register_class(c"SuperLightNativeCallbacks", &[
            (c"settings:", settings as *const c_void), (c"pause:", pause as *const c_void),
            (c"reconnect:", reconnect as *const c_void), (c"permissions:", permissions as *const c_void),
            (c"quit:", quit as *const c_void), (c"foreground:", foreground_changed as *const c_void),
            (c"sleeping:", sleeping as *const c_void), (c"awake:", awake as *const c_void),
        ])?;
        let delegate: Id = msg0(delegate_class, c"new");
        let bar: Id = msg0(class(c"NSStatusBar"), c"systemStatusBar");
        let status: Id = msg1(bar, c"statusItemWithLength:", -1f64);
        let _: Id = msg0(status, c"retain");
        let button: Id = msg0(status, c"button");
        msg1::<_, ()>(button, c"setTitle:", string("SL")?.0);
        msg1::<_, ()>(button, c"setToolTip:", string("SuperLight mouse controls")?.0);
        let menu: Id = msg0(class(c"NSMenu"), c"new");
        let mut pause_item = ptr::null_mut();
        for (title, action) in [("Settings...", c"settings:"), ("Pause remapping", c"pause:"), ("Reconnect mouse", c"reconnect:"), ("Input permissions...", c"permissions:"), ("Quit SuperLight", c"quit:")] {
            let title = string(title)?;
            let key = string("")?;
            let item: Id = msg3(msg0::<Id>(class(c"NSMenuItem"), c"alloc"), c"initWithTitle:action:keyEquivalent:", title.0, selector(action), key.0);
            msg1::<_, ()>(item, c"setTarget:", delegate);
            msg1::<_, ()>(menu, c"addItem:", item);
            if action == c"pause:" { pause_item = item; }
            msg0::<()>(item, c"release");
        }
        msg1::<_, ()>(status, c"setMenu:", menu);
        msg0::<()>(menu, c"release");
        let workspace: Id = msg0(class(c"NSWorkspace"), c"sharedWorkspace");
        let center: Id = msg0(workspace, c"notificationCenter");
        for (name, action) in [
            ("NSWorkspaceDidActivateApplicationNotification", c"foreground:"),
            ("NSWorkspaceWillSleepNotification", c"sleeping:"),
            ("NSWorkspaceSessionDidResignActiveNotification", c"sleeping:"),
            ("NSWorkspaceDidWakeNotification", c"awake:"),
            ("NSWorkspaceSessionDidBecomeActiveNotification", c"awake:"),
        ] {
            msg4::<_, _, _, _, ()>(center, c"addObserver:selector:name:object:", delegate, selector(action), string(name)?.0, ptr::null_mut::<c_void>());
        }
        let mut context = SourceContext { version: 0, info: ptr::null_mut(), retain: None, release: None, copy_description: None, equal: None, hash: None, schedule: None, cancel: None, perform: Some(perform) };
        let source = Owned::new(CFRunLoopSourceCreate(ptr::null(), 0, &mut context))?;
        let loop_ref = CFRunLoopGetMain();
        CFRunLoopAddSource(loop_ref, source.0, kCFRunLoopCommonModes);
        *LOOP_SOURCE.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some((source.0 as usize, loop_ref as usize));
        Ok(Self { app, delegate, status, center, pause_item, tap: None, tap_source: None, source, last_permissions: None })
    }

    fn request_permissions(&mut self) {
        unsafe {
            CGRequestListenEventAccess();
            CGRequestPostEventAccess();
            if let Ok(options) = dictionary() {
                CFDictionarySetValue(options.0, kAXTrustedCheckOptionPrompt, kCFBooleanTrue);
                AXIsProcessTrustedWithOptions(options.0);
            }
        }
        self.refresh_permissions();
    }

    fn refresh_permissions(&mut self) {
        let Some(shared) = SHARED.get() else { return; };
        let listen = unsafe { CGPreflightListenEventAccess() };
        let inject = unsafe { CGPreflightPostEventAccess() || AXIsProcessTrusted() };
        if listen && inject && self.tap.is_none() && !shared.stopping() {
            unsafe {
                let mask = [5, 6, 7, 22, 25, 26, 27].iter().fold(0u64, |mask, kind| mask | (1u64 << kind));
                if let Ok(tap) = Owned::new(CGEventTapCreate(0, 0, 0, mask, tap_callback, ptr::null_mut())) {
                    if let Ok(source) = Owned::new(CFMachPortCreateRunLoopSource(ptr::null(), tap.0, 0)) {
                        TAP.with(|value| value.set(tap.0));
                        CFRunLoopAddSource(CFRunLoopGetMain(), source.0, kCFRunLoopCommonModes);
                        CGEventTapEnable(tap.0, 1);
                        self.tap_source = Some(source);
                        self.tap = Some(tap);
                    } else { CFMachPortInvalidate(tap.0); }
                }
            }
        }
        let ready = listen && inject && self.tap.is_some();
        let permissions = Permissions { listen, inject, description: if ready { "Accessibility and Input Monitoring are available".into() } else { "Allow SuperLight in System Settings > Privacy & Security > Accessibility and Input Monitoring. Restart SuperLight after changing permissions if the event tap remains unavailable.".into() } };
        if self.last_permissions.as_ref() != Some(&permissions) || shared.native_ready.load(Ordering::Acquire) != ready {
            if shared.command(Command::Native { ready, permissions: permissions.clone() }) { self.last_permissions = Some(permissions); }
        }
        unsafe { msg1::<_, ()>(self.pause_item, c"setState:", isize::from(shared.policy.load().paused)); }
    }
}

impl Drop for Native {
    fn drop(&mut self) {
        *LOOP_SOURCE.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
        unsafe {
            TAP.with(|value| value.set(ptr::null()));
            if let Some(tap) = &self.tap { CGEventTapEnable(tap.0, 0); CFMachPortInvalidate(tap.0); }
            if let Some(source) = &self.tap_source { CFRunLoopRemoveSource(CFRunLoopGetMain(), source.0, kCFRunLoopCommonModes); }
            CFRunLoopSourceInvalidate(self.source.0);
            CFRunLoopRemoveSource(CFRunLoopGetMain(), self.source.0, kCFRunLoopCommonModes);
            msg1::<_, ()>(self.center, c"removeObserver:", self.delegate);
            let bar: Id = msg0(class(c"NSStatusBar"), c"systemStatusBar");
            msg1::<_, ()>(bar, c"removeStatusItem:", self.status);
            msg0::<()>(self.status, c"release");
            msg0::<()>(self.delegate, c"release");
        }
        HOOK.with(|hook| { hook.borrow_mut().take(); });
    }
}

pub fn run(shared: Arc<Shared>) -> io::Result<()> {
    let _pool = Pool::new();
    SHARED.set(Arc::clone(&shared)).map_err(|_| io::Error::other("The native event loop has already been initialized"))?;
    HOOK.with(|hook| { *hook.borrow_mut() = Some(Hook::new(Arc::clone(&shared))); });
    let mut native = unsafe { Native::new()? };
    native.refresh_permissions();
    let app = native.app;
    NATIVE.with(|slot| { *slot.borrow_mut() = Some(native); });
    post(UiEvent::Refresh);
    if !shared.stopping() { unsafe { msg0::<()>(app, c"run"); } }
    NATIVE.with(|slot| { slot.borrow_mut().take(); });
    shared.stop();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_button_numbers_and_modifiers_match_v36() {
        assert_eq!(button_source(2), Some(0));
        assert_eq!(button_source(3), Some(2));
        assert_eq!(button_source(4), Some(3));
        assert_eq!(button_source(0), None);
        assert_eq!(modifier(55) | modifier(56), (1 << 20) | (1 << 17));
        assert_eq!(modifier(0), 0);
    }

    #[test]
    fn framework_objects_release_without_an_input_permission_prompt() {
        for _ in 0..1000 {
            let value = string("MX Master 3S").unwrap();
            assert_eq!(unsafe { text(value.0) }, "MX Master 3S");
            let value = number(0x46d).unwrap();
            assert_eq!(unsafe { integer(value.0) }, 0x46d);
        }
    }
}
''')

path = Path("crates/superlight-service/src/runtime.rs")
text = path.read_text()
needle = "self.refresh_foreground();\n                if let Some(child)"
if needle in text:
    text = text.replace(needle, "self.refresh_foreground();\n                if !self.shared.headless { native::post(native::UiEvent::Refresh); }\n                if let Some(child)")
path.write_text(text)
