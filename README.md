# SuperLight

SuperLight is a native Rust Logitech HID++ mouse remapper for macOS, Windows and Linux. This rewrite targets the behavior available in upstream Mouser v3.6.0 while replacing the Python, Qt and PyInstaller runtime with a small always-on service and an on-demand native settings process.

The compatibility reference is `TomBadash/Mouser@34d93f70b2a84e425698e0d3d748cae2c5d18911`.

## Design

`superlight` owns HID++, input interception, remapping, configuration and local IPC. `superlight-ui` is a separate settings process. Closing the settings window releases its GUI memory without stopping mouse remapping.

The service uses bounded queues, fixed-size input buffers, atomic policy swaps and explicit release-on-error behavior. HID++ writes are restricted to discovered Logitech mouse capabilities. Receiver slots are correlated with replies before configuration writes are allowed.

No telemetry, cloud service, web runtime or Logitech account is used.

## v3.6 compatibility target

The rewrite covers the v3.6 behavior used by the application:

- Logitech HID++ Bluetooth and receiver discovery
- Logi Bolt receiver slots
- button remapping
- horizontal wheel actions and scroll inversion
- gesture button and RawXY gestures
- application profiles
- custom shortcuts
- media and desktop actions
- DPI read/write and DPI cycling
- SmartShift and ratchet/free-spin switching
- battery status
- start at login
- macOS Accessibility/Input Monitoring handling
- macOS trackpad filtering
- sleep, wake, reconnect and fail-open cleanup
- migration of existing v1 through v9 configuration, including unknown fields

The settings UI intentionally stays basic. It keeps the mouse-oriented interaction while leaving visual theming for later work.

## Build

Rust `1.98.1` is pinned in `rust-toolchain.toml`.

```bash
cargo build --locked --release -p superlight-service -p superlight-ui --bins
```

Run the service:

```bash
target/release/superlight
```

Open settings while the service is running:

```bash
target/release/superlight --settings
```

Useful service commands:

```text
--background
--headless
--settings
--status
--apply FILE
--pause
--resume
--reconnect
--refresh
--permissions
--quit
--version
```

## Tests

The original repository already had tests. Before the rewrite, all 261 upstream v3.6.0 tests passed on the pinned compatibility commit. `compatibility/test_v36.py` characterizes the original implementation and differential CI runs the same contracts against Rust.

Run the Rust suite:

```bash
cargo test --locked --workspace --all-targets
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

The GitHub Actions matrix runs macOS Apple Silicon, macOS Intel, Windows and Linux. It also checks service lifecycle behavior without hardware, renders the settings UI on Linux, creates optimized binaries and records idle resource measurements.

Real Logitech hardware is still required before calling a release hardware-verified. CI cannot prove Bluetooth/Bolt reconnect behavior, physical button diversion or permission recovery without attached devices.

## Platform setup

### macOS

Allow SuperLight in System Settings > Privacy & Security > Accessibility and Input Monitoring. The release bundle uses a menu-bar service and a separate settings executable.

### Windows

SuperLight uses native low-level mouse input and `SendInput`. A non-elevated process intentionally does not control elevated applications or secure desktops.

### Linux

SuperLight uses HID++, evdev and uinput. Install the packaged permission rule once:

```bash
sudo ./permissions/install-linux-permissions.sh
```

Reconnect the mouse after installing the rule. On native Wayland applications, the default profile is used when the desktop does not expose a portable foreground-application API.

## Packaging

Build first, then run:

```bash
python scripts/package_release.py --output dist
```

The package contains only the two native executables, required platform assets, this documentation, the MIT license and dependency license notices. Release archives include SHA-256 checksums. macOS local packages are ad-hoc signed unless a distribution signing/notarization process is added externally.

## Rewrite evidence and risk log

See `docs/RUST_REWRITE.md` for the immutable baseline, compatibility boundaries, automated evidence and remaining physical-hardware release gates.

## License

MIT. See `LICENSE`.
