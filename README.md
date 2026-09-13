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
