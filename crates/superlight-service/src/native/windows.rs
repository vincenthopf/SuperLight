use super::{UiEvent, login};
use crate::{
    hook::Hook,
    shared::{Command, Input, Shared},
};
use std::{
    cell::{Cell, RefCell},
    ffi::OsStr,
    io,
    mem::{size_of, zeroed},
    os::windows::ffi::OsStrExt,
    path::Path,
    ptr,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU32, AtomicUsize, Ordering},
    },
    time::Duration,
};
use superlight_core::{
    actions::{Chord, Platform},
    input::INJECTION_MARKER,
};
use superlight_ipc::{AppInfo, Permissions, Request};
use windows_sys::Win32::{
    Foundation::*,
    Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation},
    System::{
        Console::{ATTACH_PARENT_PROCESS, AttachConsole},
        LibraryLoader::GetModuleHandleW,
        Registry::*,
        RemoteDesktop::*,
        Threading::*,
    },
    UI::{Input::KeyboardAndMouse::*, Shell::*, WindowsAndMessaging::*},
};

const CONTROL_MESSAGE: u32 = WM_APP + 1;
const TRAY_MESSAGE: u32 = WM_APP + 2;
static SHARED: OnceLock<Arc<Shared>> = OnceLock::new();
static WINDOW: AtomicUsize = AtomicUsize::new(0);
static EVENTS: AtomicU32 = AtomicU32::new(0);
static FOCUS_PID: AtomicU32 = AtomicU32::new(0);
static TASKBAR_MESSAGE: AtomicU32 = AtomicU32::new(0);
thread_local! {
    static HOOK: RefCell<Option<Hook>> = const { RefCell::new(None) };
    static LAST_POINT: Cell<Option<(i32, i32)>> = const { Cell::new(None) };
}

fn wide(value: impl AsRef<OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain(Some(0)).collect()
}

pub fn attach_console() {
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

pub fn error_dialog(message: &str) {
    unsafe {
        MessageBoxW(
            ptr::null_mut(),
            wide(message).as_ptr(),
            wide("SuperLight").as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}

fn send(events: &[INPUT]) -> io::Result<()> {
    if events.is_empty() {
        return Ok(());
    }
    let count = unsafe {
        SendInput(
            events.len() as u32,
            events.as_ptr(),
            size_of::<INPUT>() as i32,
        )
    };
    if count == events.len() as u32 {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "Windows accepted {count} of {} input events. Input into elevated or secure applications is restricted.",
                events.len()
            ),
        ))
    }
}

fn mouse_event(flags: u32, data: u32) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: data,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: INJECTION_MARKER as usize,
            },
        },
    }
}

pub fn mouse(button: u8, down: bool) -> io::Result<()> {
    let flags = match (button, down) {
        (0, true) => MOUSEEVENTF_LEFTDOWN,
        (0, false) => MOUSEEVENTF_LEFTUP,
        (1, true) => MOUSEEVENTF_RIGHTDOWN,
        (1, false) => MOUSEEVENTF_RIGHTUP,
        (2, true) => MOUSEEVENTF_MIDDLEDOWN,
        (2, false) => MOUSEEVENTF_MIDDLEUP,
        (3 | 4, true) => MOUSEEVENTF_XDOWN,
        (3 | 4, false) => MOUSEEVENTF_XUP,
        _ => return Err(io::Error::other("Invalid mouse button")),
    };
    send(&[mouse_event(
        flags,
        if button >= 3 {
            u32::from(button - 2)
        } else {
            0
        },
    )])
}

pub fn scroll(horizontal: bool, delta: i32) -> io::Result<()> {
    send(&[mouse_event(
        if horizontal {
            MOUSEEVENTF_HWHEEL
        } else {
            MOUSEEVENTF_WHEEL
        },
        delta as u32,
    )])
}

fn modifier(code: u16) -> bool {
    matches!(code, 0x10..=0x12 | 0x5b..=0x5c | 0xa0..=0xa5)
}

fn key_event(key: u16, up: bool) -> INPUT {
    let scan = unsafe { MapVirtualKeyW(u32::from(key), MAPVK_VK_TO_VSC_EX) };
    let mut flags = if up { KEYEVENTF_KEYUP } else { 0 };
    if scan & 0xff00 != 0 {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                wScan: scan as u16,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: INJECTION_MARKER as usize,
            },
        },
    }
}

pub fn chord(chord: Chord) -> io::Result<()> {
    let keys = chord
        .codes
        .get(..usize::from(chord.len))
        .filter(|keys| !keys.is_empty())
        .ok_or_else(|| io::Error::other("Invalid shortcut"))?;
    let mut pressed = [0u16; 8];
    let mut len = 0;
    for &key in keys {
        if modifier(key) && unsafe { GetAsyncKeyState(i32::from(key)) } < 0 {
            continue;
        }
        pressed[len] = key;
        len += 1;
    }
    let keys = &pressed[..len];
    let mut events: [INPUT; 16] = unsafe { zeroed() };
    if chord.phased {
        for &key in keys {
            if let Err(error) = send(&[key_event(key, false)]) {
                for &key in keys.iter().rev() {
                    let _ = send(&[key_event(key, true)]);
                }
                return Err(error);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let mut result = Ok(());
        for &key in keys.iter().rev() {
            if let Err(error) = send(&[key_event(key, true)]) {
                result = Err(error);
            }
        }
        return result;
    }
    for (index, &key) in keys.iter().enumerate() {
        events[index] = key_event(key, false);
        events[2 * len - index - 1] = key_event(key, true);
    }
    if let Err(error) = send(&events[..2 * len]) {
        for (index, &key) in keys.iter().rev().enumerate() {
            events[index] = key_event(key, true);
        }
        let _ = send(&events[..len]);
        return Err(error);
    }
    Ok(())
}

pub fn media(key: u8) -> io::Result<()> {
    let key = [
        VK_VOLUME_UP,
        VK_VOLUME_DOWN,
        VK_VOLUME_MUTE,
        VK_MEDIA_PLAY_PAUSE,
        VK_MEDIA_NEXT_TRACK,
        VK_MEDIA_PREV_TRACK,
    ]
    .get(usize::from(key))
    .copied()
    .ok_or_else(|| io::Error::other("Invalid media key"))?;
    chord(Chord {
        codes: [key, 0, 0, 0, 0, 0, 0, 0],
        len: 1,
        phased: false,
    })
}

pub fn system(action: u8) -> io::Result<()> {
    let combo = match action {
        0 => "super+tab",
        1 => "alt+tab",
        2 => "super+d",
        3 => "super",
        4 => "ctrl+super+left",
        5 => "ctrl+super+right",
        _ => return Err(io::Error::other("Invalid system action")),
    };
    chord(Chord::parse(combo, Platform::Windows).map_err(io::Error::other)?)
}

struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

fn elevated(process: HANDLE) -> Option<bool> {
    unsafe {
        let mut token = ptr::null_mut();
        if OpenProcessToken(process, TOKEN_QUERY, &mut token) == 0 {
            return None;
        }
        let token = Handle(token);
        let mut elevation: TOKEN_ELEVATION = zeroed();
        let mut returned = 0;
        (GetTokenInformation(
            token.0,
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        ) != 0)
            .then_some(elevation.TokenIsElevated != 0)
    }
}

pub fn foreground() -> AppInfo {
    unsafe {
        let window = GetForegroundWindow();
        if window.is_null() {
            return AppInfo {
                input_restricted: true,
                ..AppInfo::default()
            };
        }
        let mut pid = 0;
        GetWindowThreadProcessId(window, &mut pid);
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return AppInfo {
                pid,
                input_restricted: true,
                ..AppInfo::default()
            };
        }
        let process = Handle(process);
        let mut path = [0u16; 4096];
        let mut len = path.len() as u32;
        if QueryFullProcessImageNameW(process.0, 0, path.as_mut_ptr(), &mut len) == 0 {
            return AppInfo {
                pid,
                input_restricted: true,
                ..AppInfo::default()
            };
        }
        let path = String::from_utf16_lossy(&path[..len as usize]);
        let id = Path::new(&path)
            .file_name()
            .and_then(OsStr::to_str)
            .unwrap_or_default()
            .to_owned();
        let name = Path::new(&id)
            .file_stem()
            .and_then(OsStr::to_str)
            .unwrap_or(&id)
            .to_owned();
        let input_restricted =
            elevated(process.0).unwrap_or(true) && !elevated(GetCurrentProcess()).unwrap_or(false);
        AppInfo {
            pid,
            name: name.clone(),
            id: id.clone(),
            aliases: vec![id, name, path],
            input_restricted,
        }
    }
}

pub fn set_start_at_login(enabled: bool, executable: &Path) -> io::Result<()> {
    let executable = login::checked_executable(executable)?;
    unsafe {
        let mut key = ptr::null_mut();
        let status = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            wide("Software\\Microsoft\\Windows\\CurrentVersion\\Run").as_ptr(),
            0,
            ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            ptr::null(),
            &mut key,
            ptr::null_mut(),
        );
        if status != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(status as i32));
        }
        let name = wide("SuperLight");
        let status = if enabled {
            let value = wide(format!("\"{executable}\" --background"));
            RegSetValueExW(
                key,
                name.as_ptr(),
                0,
                REG_SZ,
                value.as_ptr().cast(),
                (value.len() * size_of::<u16>()) as u32,
            )
        } else {
            RegDeleteValueW(key, name.as_ptr())
        };
        RegCloseKey(key);
        if status == ERROR_SUCCESS || (!enabled && status == ERROR_FILE_NOT_FOUND) {
            Ok(())
        } else {
            Err(io::Error::from_raw_os_error(status as i32))
        }
    }
}

pub fn post(event: UiEvent) {
    let flag = match event {
        UiEvent::Quit => 1,
        UiEvent::Permissions => 2,
        UiEvent::Refresh => 4,
        UiEvent::Focus(pid) => {
            FOCUS_PID.store(pid, Ordering::Release);
            8
        }
    };
    EVENTS.fetch_or(flag, Ordering::AcqRel);
    let window = WINDOW.load(Ordering::Acquire) as HWND;
    if !window.is_null() {
        unsafe {
            PostMessageW(window, CONTROL_MESSAGE, 0, 0);
        }
    }
}

fn movement_delta(x: i32, y: i32) -> Option<(f64, f64)> {
    LAST_POINT.with(|previous| {
        previous.replace(Some((x, y))).map(|(old_x, old_y)| {
            (
                f64::from(x.saturating_sub(old_x)),
                f64::from(y.saturating_sub(old_y)),
            )
        })
    })
}

fn process_mouse(kind: u32, event: &MSLLHOOKSTRUCT) -> bool {
    if event.flags & LLMHF_INJECTED != 0 || event.dwExtraInfo == INJECTION_MARKER as usize {
        return false;
    }
    let Some(shared) = SHARED.get() else {
        return false;
    };
    HOOK.with(|hook| {
        let Ok(mut hook) = hook.try_borrow_mut() else {
            return false;
        };
        let Some(hook) = hook.as_mut() else {
            return false;
        };
        match kind {
            WM_MBUTTONDOWN | WM_MBUTTONUP => hook.button(0, kind == WM_MBUTTONDOWN),
            WM_XBUTTONDOWN | WM_XBUTTONUP => {
                let button = event.mouseData >> 16;
                if button == 1 || button == 2 {
                    hook.button((button + 1) as usize, kind == WM_XBUTTONDOWN)
                } else {
                    false
                }
            }
            WM_MOUSEMOVE => {
                movement_delta(event.pt.x, event.pt.y).is_some_and(|(x, y)| hook.movement(x, y))
            }
            WM_MOUSEWHEEL | WM_MOUSEHWHEEL if shared.allowed() => {
                let horizontal = kind == WM_MOUSEHWHEEL;
                let delta = i32::from((event.mouseData >> 16) as i16);
                if horizontal
                    && delta != 0
                    && hook.wheel(if delta > 0 { 5 } else { 4 }, f64::from(delta) / 120.0)
                {
                    return true;
                }
                let policy = shared.policy.load();
                let invert = if horizontal {
                    policy.invert_horizontal
                } else {
                    policy.invert_vertical
                };
                invert
                    && shared.emit(Input::Scroll {
                        horizontal,
                        delta: delta.saturating_neg(),
                    })
            }
            _ => false,
        }
    })
}

unsafe extern "system" fn mouse_hook(code: i32, kind: WPARAM, data: LPARAM) -> LRESULT {
    if code >= 0 && data != 0 {
        let blocked = std::panic::catch_unwind(|| {
            process_mouse(kind as u32, unsafe { &*(data as *const MSLLHOOKSTRUCT) })
        })
        .unwrap_or_else(|_| {
            if let Some(shared) = SHARED.get() {
                shared.release_all();
            }
            false
        });
        if blocked {
            return 1;
        }
    }
    unsafe { CallNextHookEx(ptr::null_mut(), code, kind, data) }
}

fn tray_data(window: HWND) -> NOTIFYICONDATAW {
    let mut data: NOTIFYICONDATAW = unsafe { zeroed() };
    data.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
    data.hWnd = window;
    data.uID = 1;
    data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    data.uCallbackMessage = TRAY_MESSAGE;
    data.hIcon = unsafe { LoadIconW(ptr::null_mut(), IDI_APPLICATION) };
    let tip = wide("SuperLight mouse controls");
    let len = tip.len().min(data.szTip.len());
    data.szTip[..len].copy_from_slice(&tip[..len]);
    data
}

fn issue(request: Request) {
    if let Some(shared) = SHARED.get() {
        shared.command(Command::Request(request));
    }
}

fn menu(window: HWND) {
    unsafe {
        let menu = CreatePopupMenu();
        if menu.is_null() {
            return;
        }
        let paused = SHARED
            .get()
            .is_some_and(|shared| shared.policy.load().paused);
        for (id, title) in [
            (1, "Settings..."),
            (2, "Pause remapping"),
            (3, "Reconnect mouse"),
            (4, "Input permissions..."),
            (5, "Quit SuperLight"),
        ] {
            AppendMenuW(
                menu,
                MF_STRING | if id == 2 && paused { MF_CHECKED } else { 0 },
                id,
                wide(title).as_ptr(),
            );
        }
        let mut point = POINT { x: 0, y: 0 };
        GetCursorPos(&mut point);
        SetForegroundWindow(window);
        let selected = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON,
            point.x,
            point.y,
            0,
            window,
            ptr::null(),
        );
        DestroyMenu(menu);
        match selected {
            1 => issue(Request::ShowSettings),
            2 => issue(Request::SetPaused { value: !paused }),
            3 => issue(Request::Reconnect),
            4 => post(UiEvent::Permissions),
            5 => {
                if let Some(shared) = SHARED.get() {
                    shared.stop();
                }
            }
            _ => {}
        }
        PostMessageW(window, WM_NULL, 0, 0);
    }
}

unsafe extern "system" fn focus_window(window: HWND, pid: LPARAM) -> i32 {
    unsafe {
        let mut actual = 0;
        GetWindowThreadProcessId(window, &mut actual);
        if actual == pid as u32 && IsWindowVisible(window) != 0 {
            ShowWindow(window, SW_RESTORE);
            SetForegroundWindow(window);
            return 0;
        }
    }
    1
}

fn process_message(window: HWND, message: u32, word: WPARAM, data: LPARAM) -> Option<LRESULT> {
    match message {
        CONTROL_MESSAGE => {
            let events = EVENTS.swap(0, Ordering::AcqRel);
            if events & 2 != 0 {
                if let Some(shared) = SHARED.get() {
                    shared.report("Windows does not require an input permission prompt. Remapping cannot control elevated applications or secure desktops from a non-elevated process.");
                }
                issue(Request::ShowSettings);
            }
            if events & 8 != 0 {
                let pid = FOCUS_PID.load(Ordering::Acquire);
                unsafe {
                    AllowSetForegroundWindow(pid);
                    EnumWindows(Some(focus_window), pid as LPARAM);
                }
            }
            if events & 1 != 0 {
                unsafe {
                    PostQuitMessage(0);
                }
            }
            Some(0)
        }
        TRAY_MESSAGE => {
            if data as u32 == WM_LBUTTONUP || data as u32 == WM_LBUTTONDBLCLK {
                issue(Request::ShowSettings);
            }
            if data as u32 == WM_RBUTTONUP || data as u32 == WM_CONTEXTMENU {
                menu(window);
            }
            Some(0)
        }
        WM_CLOSE => {
            if let Some(shared) = SHARED.get() {
                shared.stop();
            }
            Some(0)
        }
        WM_POWERBROADCAST => {
            if let Some(shared) = SHARED.get() {
                if word == 4 {
                    shared.suspended.store(true, Ordering::Release);
                    shared.request_reconnect();
                }
                if word == 7 || word == 18 {
                    shared.suspended.store(false, Ordering::Release);
                    shared.request_reconnect();
                }
            }
            Some(1)
        }
        WM_WTSSESSION_CHANGE => {
            if let Some(shared) = SHARED.get() {
                if word == WTS_SESSION_LOCK as usize {
                    shared.suspended.store(true, Ordering::Release);
                    shared.request_reconnect();
                }
                if word == WTS_SESSION_UNLOCK as usize {
                    shared.suspended.store(false, Ordering::Release);
                    shared.request_reconnect();
                }
            }
            Some(0)
        }
        _ if message == TASKBAR_MESSAGE.load(Ordering::Acquire) => {
            unsafe {
                Shell_NotifyIconW(NIM_ADD, &tray_data(window));
            }
            Some(0)
        }
        _ => None,
    }
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    word: WPARAM,
    data: LPARAM,
) -> LRESULT {
    match std::panic::catch_unwind(|| process_message(window, message, word, data)) {
        Ok(Some(result)) => result,
        Ok(None) => unsafe { DefWindowProcW(window, message, word, data) },
        Err(_) => {
            if let Some(shared) = SHARED.get() {
                shared.release_all();
            }
            unsafe { DefWindowProcW(window, message, word, data) }
        }
    }
}

struct Native {
    window: HWND,
    hook: HHOOK,
    instance: HINSTANCE,
    class: Vec<u16>,
}
impl Drop for Native {
    fn drop(&mut self) {
        WINDOW.store(0, Ordering::Release);
        unsafe {
            if !self.hook.is_null() {
                UnhookWindowsHookEx(self.hook);
            }
            WTSUnRegisterSessionNotification(self.window);
            Shell_NotifyIconW(NIM_DELETE, &tray_data(self.window));
            DestroyWindow(self.window);
            UnregisterClassW(self.class.as_ptr(), self.instance);
        }
        HOOK.with(|hook| *hook.borrow_mut() = None);
        LAST_POINT.with(|point| point.set(None));
    }
}

pub fn run(shared: Arc<Shared>) -> io::Result<()> {
    SHARED
        .set(Arc::clone(&shared))
        .map_err(|_| io::Error::other("Native input is already running"))?;
    unsafe {
        let instance = GetModuleHandleW(ptr::null());
        let class = wide("SuperLightNativeWindow");
        let mut definition: WNDCLASSW = zeroed();
        definition.lpfnWndProc = Some(window_proc);
        definition.hInstance = instance;
        definition.lpszClassName = class.as_ptr();
        if RegisterClassW(&definition) == 0 {
            return Err(io::Error::last_os_error());
        }
        let window = CreateWindowExW(
            0,
            class.as_ptr(),
            wide("SuperLight").as_ptr(),
            0,
            0,
            0,
            0,
            0,
            ptr::null_mut(),
            ptr::null_mut(),
            instance,
            ptr::null(),
        );
        if window.is_null() {
            UnregisterClassW(class.as_ptr(), instance);
            return Err(io::Error::last_os_error());
        }
        let mut native = Native {
            window,
            hook: ptr::null_mut(),
            instance,
            class,
        };
        WINDOW.store(window as usize, Ordering::Release);
        TASKBAR_MESSAGE.store(
            RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()),
            Ordering::Release,
        );
        if Shell_NotifyIconW(NIM_ADD, &tray_data(window)) == 0 {
            return Err(io::Error::other("Could not add the SuperLight tray icon"));
        }
        WTSRegisterSessionNotification(window, NOTIFY_FOR_THIS_SESSION);
        HOOK.with(|hook| *hook.borrow_mut() = Some(Hook::new(Arc::clone(&shared))));
        native.hook = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), instance, 0);
        let ready = !native.hook.is_null();
        shared.command(Command::Native { ready, permissions: Permissions { listen: ready, inject: ready, description: if ready { "Native remapping is available. Elevated applications and secure desktops remain protected by Windows." } else { "Windows did not allow the mouse hook. Input has not been intercepted." }.into() } });
        post(UiEvent::Refresh);
        let mut message: MSG = zeroed();
        loop {
            let result = GetMessageW(&mut message, ptr::null_mut(), 0, 0);
            if result == 0 {
                break;
            }
            if result < 0 {
                return Err(io::Error::last_os_error());
            }
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        drop(native);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn movement_delta_uses_the_previous_hook_position() {
        LAST_POINT.with(|point| point.set(None));
        assert_eq!(movement_delta(10, 20), None);
        assert_eq!(movement_delta(15, 17), Some((5.0, -3.0)));
    }
}
