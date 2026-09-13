#![cfg_attr(windows, windows_subsystem = "windows")]

use std::{io, path::PathBuf};
use superlight_ipc::{Paths, Request, Response, call, store::read_limited};
use superlight_service::{Options, native};

fn request(request: Request) -> io::Result<Response> {
    let response = call(&Paths::discover()?, &request)?;
    if response.ok {
        Ok(response)
    } else {
        Err(io::Error::other(
            response.error.unwrap_or_else(|| "Operation failed".into()),
        ))
    }
}

fn diagnostics(snapshot: &superlight_ipc::Snapshot) -> serde_json::Value {
    serde_json::json!({
        "schema_version": 1,
        "version": env!("CARGO_PKG_VERSION"),
        "os": std::env::consts::OS,
        "architecture": std::env::consts::ARCH,
        "native_ready": snapshot.native_ready,
        "paused": snapshot.paused,
        "suspended": snapshot.suspended,
        "permissions": {
            "listen": snapshot.permissions.listen,
            "inject": snapshot.permissions.inject
        },
        "dropped_events": snapshot.dropped_events,
        "device": snapshot.device.as_ref().map(|device| serde_json::json!({
            "model": device.model_key,
            "product_id": format!("0x{:04x}", device.product_id),
            "transport": device.transport,
            "backend": device.backend,
            "receiver_slot": device.receiver_slot,
            "dpi": device.dpi,
            "dpi_range": [device.dpi_min, device.dpi_max],
            "smart_shift": device.smart_shift,
            "battery": device.battery,
            "supports_dpi": device.supports_dpi,
            "supports_smart_shift": device.supports_smart_shift,
            "supports_gesture": device.supports_gesture,
            "supports_mode_shift": device.supports_mode_shift,
            "supports_dpi_switch": device.supports_dpi_switch,
            "raw_xy": device.raw_xy,
            "controls": device.controls
        }))
    })
}

fn execute() -> io::Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let strings: Vec<_> = args.iter().map(|arg| arg.to_string_lossy()).collect();
    if strings.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!(
            "SuperLight\n\nRun without arguments to start the service.\n--background  Start without opening settings\n--headless    Verify control and configuration without native input or HID\n--settings    Open the settings window\n--status      Print the current state as JSON\n--diagnostics Print shareable device diagnostics without profiles or app paths\n--apply FILE  Validate and save a complete configuration\n--pause       Pause remapping and release captured input\n--resume      Resume remapping\n--reconnect   Reopen the Logitech connection\n--refresh     Read hardware settings\n--permissions Request native input permissions\n--quit        Stop the service\n--version     Print the version"
        );
        return Ok(());
    }
    if strings.iter().any(|arg| arg == "--version" || arg == "-V") {
        println!("SuperLight {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if strings.first().is_some_and(|arg| arg == "--diagnostics") {
        if args.len() != 1 {
            return Err(io::Error::other("Usage: superlight --diagnostics"));
        }
        let snapshot = request(Request::Get)?
            .snapshot
            .ok_or_else(|| io::Error::other("Missing service state"))?;
        println!(
            "{}",
            serde_json::to_string_pretty(&diagnostics(&snapshot)).map_err(io::Error::other)?
        );
        return Ok(());
    }
    if let Some(index) = strings.iter().position(|arg| arg == "--apply") {
        if args.len() != 2 || index != 0 {
            return Err(io::Error::other("Usage: superlight --apply FILE"));
        }
        let bytes = read_limited(&PathBuf::from(&args[1]), superlight_core::CONFIG_LIMIT)?;
        let config = superlight_core::config::parse(&bytes).map_err(io::Error::other)?;
        let snapshot = request(Request::Get)?
            .snapshot
            .ok_or_else(|| io::Error::other("Missing service state"))?;
        let response = request(Request::Apply {
            expected_revision: snapshot.revision,
            config,
        })?;
        println!(
            "{}",
            serde_json::to_string(&response).map_err(io::Error::other)?
        );
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
            if args.len() != 1 {
                return Err(io::Error::other("Unexpected command arguments"));
            }
            let response = request(command)?;
            println!(
                "{}",
                serde_json::to_string(&response).map_err(io::Error::other)?
            );
            return Ok(());
        }
    }
    let mut options = Options::default();
    for arg in &strings {
        match arg.as_ref() {
            "--background" => options.background = true,
            "--headless" => options.headless = true,
            "--settings" => options.force_ui = true,
            _ => {
                return Err(io::Error::other(format!(
                    "Unknown option: {arg}. Use --help."
                )));
            }
        }
    }
    let paths = Paths::discover()?;
    if let Ok(response) = call(&paths, &Request::Get)
        && response.ok
    {
        if !options.background && !options.headless {
            request(Request::ShowSettings)?;
        }
        return Ok(());
    }
    hidapi::HidApi::disable_device_discovery();
    superlight_service::run(options)
}

fn main() {
    native::attach_console();
    if let Err(error) = execute() {
        eprintln!("SuperLight: {error}");
        if std::env::args_os().len() == 1 {
            native::error_dialog(&error.to_string());
        }
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_exclude_configuration_foreground_and_free_text() {
        let private = "PRIVATE_PATH_OR_SHORTCUT";
        let mut snapshot = superlight_ipc::Snapshot {
            config: serde_json::json!({"custom": private}),
            active_profile: private.into(),
            errors: vec![private.into()],
            notice: Some(private.into()),
            ..Default::default()
        };
        snapshot.foreground.name = private.into();
        snapshot.foreground.aliases = vec![private.into()];
        snapshot.permissions.description = private.into();
        snapshot.device = Some(superlight_ipc::DeviceStatus {
            name: private.into(),
            model_key: "mx_master_3s".into(),
            product_id: 0xb034,
            ..Default::default()
        });
        let report = diagnostics(&snapshot);
        assert!(!report.to_string().contains(private));
        assert_eq!(report["device"]["product_id"], "0xb034");
        assert_eq!(report["device"]["model"], "mx_master_3s");
        snapshot.device = None;
        assert!(diagnostics(&snapshot)["device"].is_null());
    }
}
