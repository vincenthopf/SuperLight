# SuperLight

Native Logitech mouse remapping for macOS on Apple Silicon.

- Rust background service for HID++, input, remapping and configuration.
- Native SwiftUI settings app on macOS.
- Local-only operation. No account, telemetry or cloud dependency.

## Build

Use an Apple Silicon Mac with Xcode 26 or newer. Rust is pinned by `rust-toolchain.toml`. The build script compiles the Rust service and native SwiftUI settings app:

```sh
bash macos/inspector/build.sh
bash macos/inspector/install.sh
```

The installer creates `~/Applications/SuperLight.app` and refuses to replace an existing installation. Grant it Accessibility and Input Monitoring in System Settings.

## Development checks

```sh
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace --all-targets
python3 -W error -m unittest discover -s scripts -p 'test_*.py' -v
```

Release tooling tests require Python 3.12 or newer and use isolated fixtures. They do not launch the app or change installed settings.

## Releases

Push a version tag matching the workspace version, such as `v4.0.0-alpha.2`. GitHub Actions builds the macOS Apple Silicon package, verifies its extracted executables and SHA-256 checksum, and publishes a GitHub release. Tags containing a hyphen are prereleases.

Release packaging uses Python 3.12 or newer for TOML, property lists, dependency licenses, and ZIP metadata. Python is not an application runtime dependency. Validate the workspace version with `python3 scripts/package_release.py --check-version`.

macOS packages are ad-hoc signed. Distribution signing and notarization require Apple credentials.

## License

MIT. Original Mouser attribution is preserved in `LICENSE`.

## Mouse compatibility

[The compatibility matrix](compatibility/mice.json) separates catalogue recognition from physical verification. Only the MX Master 3S over Bluetooth on macOS has local functional evidence. Other models and transports still need hardware testing. Windows, Linux, and Intel macOS are not release targets. A model name or passing synthetic fixture is not certification.

The catalogue includes MX Master, MX Anywhere, MX Vertical, M650, M585/M590 and M720 variants. Controls require runtime HID++ discovery. Missing or non-divertable controls are not advertised as gesture, mode-shift or DPI buttons. Unknown Logitech vendor interfaces are probed conservatively and must identify as a mouse or trackball. Unsupported devices are left to the OS. HID++ 1.0, G-series onboard profiles, MX Master 4 Actions Ring/haptics and simultaneous control of multiple mice are not implemented.

With the service running, collect a report using `superlight --diagnostics`. On macOS:

```sh
~/Applications/SuperLight.app/Contents/MacOS/superlight --diagnostics
```

The report excludes profiles, shortcuts, foreground applications, device names and free-text errors. Review it before posting a mouse compatibility issue. DPI ranges are catalogue limits or conservative defaults, not a measurement of every sensor's supported range.

Before marking a model/transport/platform verified for a release:

1. Record the release or commit, OS, exact model, transport and diagnostics. Quit other remappers first.
2. Test every available control, wheel, gesture, DPI, SmartShift and battery indication. Mark absent features as unavailable, not passed.
3. Test pause/resume, disconnect while holding a mapped button, 10 reconnect cycles, three sleep/wake cycles, and quitting to restore ordinary mouse behavior.
4. Run 30 minutes of active use and record failures and memory growth. This is not a leak-free certification.
5. Attach the results to the compatibility issue and update the matrix with the evidence reference. Repeat independently for Bluetooth, each receiver type and each OS tested.

CI runs synthetic discovery fixtures across receiver/direct addressing and report-ID variants on macOS ARM64. It does not replace physical device testing.
