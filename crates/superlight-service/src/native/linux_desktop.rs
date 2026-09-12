use std::{io, path::Path, sync::Mutex};
use superlight_ipc::AppInfo;
use x11rb::{connection::Connection, protocol::xproto::{AtomEnum, ClientMessageEvent, ConnectionExt, EventMask}, rust_connection::RustConnection};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
static DESKTOP: Mutex<Option<Desktop>> = Mutex::new(None);

struct Desktop {
    connection: RustConnection,
    root: u32,
    active: u32,
    pid: u32,
    clients: u32,
}

impl Desktop {
    fn open() -> Result<Self> {
        if std::env::var("XDG_SESSION_TYPE").is_ok_and(|value| value.eq_ignore_ascii_case("wayland")) {
            return Err(io::Error::new(io::ErrorKind::Unsupported, "Native Wayland applications do not expose a cross-desktop foreground application API").into());
        }
        let display = std::env::var("DISPLAY")?;
        if !display.starts_with(':') && !display.starts_with("unix:") {
            return Err(io::Error::new(io::ErrorKind::Unsupported, "Only local X11 desktop connections are used").into());
        }
        let (connection, screen) = x11rb::connect(Some(&display))?;
        let root = connection.setup().roots.get(screen).ok_or("The X11 screen is unavailable")?.root;
        let active = connection.intern_atom(false, b"_NET_ACTIVE_WINDOW")?.reply()?.atom;
        let pid = connection.intern_atom(false, b"_NET_WM_PID")?.reply()?.atom;
        let clients = connection.intern_atom(false, b"_NET_CLIENT_LIST")?.reply()?.atom;
        Ok(Self { connection, root, active, pid, clients })
    }

    fn integer(&self, window: u32, property: u32, kind: AtomEnum) -> Result<Option<u32>> {
        let reply = self.connection.get_property(false, window, property, kind, 0, 1)?.reply()?;
        Ok(reply.value32().and_then(|mut values| values.next()))
    }

    fn foreground(&self) -> Result<AppInfo> {
        let Some(window) = self.integer(self.root, self.active, AtomEnum::WINDOW)?.filter(|window| *window != 0) else {
            return Ok(AppInfo::default());
        };
        let pid = self.integer(window, self.pid, AtomEnum::CARDINAL)?.unwrap_or(0);
        let reply = self.connection.get_property(false, window, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 1024)?.reply()?;
        let mut aliases: Vec<String> = reply.value.split(|byte| *byte == 0).filter(|part| !part.is_empty()).take(2).map(|part| String::from_utf8_lossy(part).chars().take(255).collect()).collect();
        let executable = if pid == 0 { None } else { std::fs::read_link(format!("/proc/{pid}/exe")).ok() };
        if let Some(path) = &executable {
            if let Some(name) = path.file_name().and_then(|name| name.to_str()) { aliases.push(name.to_owned()); }
            if let Some(path) = path.to_str() { aliases.push(path.to_owned()); }
        }
        let id = aliases.first().cloned().unwrap_or_default();
        let name = executable.as_ref().and_then(|path| Path::new(path).file_stem()).and_then(|name| name.to_str()).unwrap_or(&id).to_owned();
        Ok(AppInfo { pid, name, id, aliases, input_restricted: false })
    }

    fn focus(&self, pid: u32) -> Result<()> {
        let reply = self.connection.get_property(false, self.root, self.clients, AtomEnum::WINDOW, 0, 1024)?.reply()?;
        for window in reply.value32().into_iter().flatten().take(1024) {
            if self.integer(window, self.pid, AtomEnum::CARDINAL)? == Some(pid) {
                let event = ClientMessageEvent::new(32, window, self.active, [2, x11rb::CURRENT_TIME, 0, 0, 0]);
                self.connection.send_event(false, self.root, EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY, event)?.check()?;
                self.connection.flush()?;
                return Ok(());
            }
        }
        Err(io::Error::new(io::ErrorKind::NotFound, "The settings window is not visible to this desktop").into())
    }
}

pub fn foreground() -> AppInfo {
    let Ok(mut desktop) = DESKTOP.lock() else { return AppInfo::default(); };
    if desktop.is_none() { *desktop = Desktop::open().ok(); }
    match desktop.as_ref().map(Desktop::foreground) {
        Some(Ok(app)) => app,
        Some(Err(_)) => { *desktop = None; AppInfo::default() }
        None => AppInfo::default(),
    }
}

pub fn focus(pid: u32) -> io::Result<()> {
    let mut desktop = DESKTOP.lock().map_err(|_| io::Error::other("The desktop connection is unavailable"))?;
    if desktop.is_none() { *desktop = Some(Desktop::open().map_err(io::Error::other)?); }
    desktop.as_ref().ok_or_else(|| io::Error::other("No local X11 desktop is available"))?.focus(pid).map_err(io::Error::other)
}
