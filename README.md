# SuperLight

Native Logitech mouse remapping for macOS Apple Silicon and Windows x64/ARM64.

- Rust background service for HID++, input, remapping and configuration.
- Native SwiftUI settings app on macOS.
- Local-only operation. No account, telemetry or cloud dependency.

## Build

Rust is pinned by `rust-toolchain.toml`.

```sh
cargo build --locked --release -p superlight-service -p superlight-ui --bins
```

On macOS, use Xcode 26 or newer:

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
```

## Releases

Push a version tag matching the workspace version, such as `v4.0.0-alpha.1`. GitHub Actions builds macOS Apple Silicon, Windows x64 and Windows ARM64 packages, includes SHA-256 checksums, and publishes a GitHub release. Tags containing a hyphen are prereleases.

macOS packages are ad-hoc signed. Distribution signing and notarization require Apple credentials.

## License

MIT. Original Mouser attribution is preserved in `LICENSE`.

## Mouse compatibility

[The compatibility matrix](compatibility/mice.json) separates catalogue recognition from physical verification. Only the MX Master 3S over Bluetooth on macOS has local functional evidence. Other models, transports and Windows still need hardware testing. A model name or passing synthetic fixture is not certification.

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

CI runs synthetic discovery fixtures across receiver/direct addressing and report-ID variants on macOS ARM64 and Windows x64/ARM64. It does not replace physical device testing.
