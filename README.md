# SuperLight

Native Logitech mouse remapping for macOS Apple Silicon and Windows x64/ARM64.

- Rust background service for HID++, input, remapping and configuration.
- Native SwiftUI settings app on macOS.
- Local-only operation. No account, telemetry or cloud dependency.

## Build

Rust is pinned by `rust-toolchain.toml`. On Windows:

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
```

## Releases

Push a version tag matching the workspace version, such as `v4.0.0-alpha.1`. GitHub Actions builds the macOS Apple Silicon package with a SHA-256 checksum and publishes a GitHub release. Windows is checked in CI but not packaged. Tags containing a hyphen are prereleases.

macOS packages are ad-hoc signed. Distribution signing and notarization require Apple credentials.

## License

MIT. Original Mouser attribution is preserved in `LICENSE`.

## Mouse compatibility

[The compatibility matrix](compatibility/mice.json) separates catalogue recognition from physical verification. Only the MX Master 3S over Bluetooth on macOS has local functional evidence. Other models, transports and Windows still need hardware testing. A model name in the catalogue is not certification.

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

CI runs lint and release builds on macOS ARM64 and Windows x64. It does not exercise devices, so it does not replace physical device testing.

## Protocol notes

These facts are not obvious from the code. Most HID++ constants are named in `crates/superlight-core/src/hidpp.rs`.

- HID++ writes are always 20-byte long reports (0x11) with software id 0x0A. Bluetooth devices reject short reports. More than 16 parameter bytes is an error and is never truncated.
- Input reports may arrive with or without the 0x10/0x11 report ID. Device indices (0xff and 1 to 6) never collide with those IDs, so both forms parse the same. macOS IOHID passes both forms through.
- Some firmware replies with function f+1. A reply with function f or f+1 completes request f. Software id 0 marks device notifications.
- Errors arrive as feature 0xff (HID++ 2.0) or 0x8f (HID++ 1.0). An error completes a request only when device, feature, function and software id all match. Errors 6, 7 and 9 from getFeature mean the feature is absent.
- Bluetooth devices use index 0xff only. Receivers are probed at 0xff and slots 1 to 6. Product IDs 0xb000 to 0xbfff are Bluetooth. Receiver product IDs (Unifying 0xc52b and 0xc532, Bolt 0xc548) are never catalogue mice, so a mouse behind a receiver resolves by name. A product ID match beats a name match.
- Device kind 3 (mouse) and 5 (trackball) are accepted. Any other reported kind is rejected even when the name matches the catalogue. If the device reports no kind, only a catalogue model is accepted.
- Discovery is read-only. Capabilities come from discovered divertable controls, not the model name. Writes need a confirmed mouse on the probed slot, an allow-listed feature, a confirmed control and a DPI value in range. Firmware update features are never reachable.
- The gesture control is chosen in order 0x00c3, then 0x00d7, unless the model overrides it (M720: 0x00d0, then 0x00d7). Any other divertable virtual control with raw XY is tried after these. Gesture divert tries raw XY (0x33) before plain divert (0x03).
- Reprog event 0 lists the held diverted controls as a snapshot, so a release is detected by absence. The three diverted controls (gesture, mode shift, DPI switch) report as button sources 1, 6 and 7. Event 1 carries raw XY motion and is used only while the gesture control is held.
- SmartShift uses mode 1 for freespin and 2 for ratchet, threshold 1 to 50, and 255 for a fixed ratchet.
- Input safety: a release always pairs with the action captured at its press, across pause and profile changes. Cancel or disconnect never produces a click or an orphan button-up. A failed press injection passes the original input through. A full input queue passes input to the OS. A failed mouse-up is retried. Held buttons are released after 20 s.
- Device actions (SmartShift, scroll mode and DPI cycling) never run HID I/O on the input thread.
- Gesture motion from the OS and from HID raw XY is never summed. A segment keeps its first source, except that on macOS the first HID event restarts a native segment as HID.
- Wheel actions count at most one step per event. The cooldown is 60 ms for volume and 350 ms for other actions.
- Default DPI presets are 800, 1200, 1600 and 2400. Presets are clamped to the model's DPI range. The default DPI of 1000 is not a preset, so the first cycle goes to the first clamped preset: 800 on most models, 1000 on the M720.
- Config files are written atomically. On Unix they get mode 0600. The mode comes from `tempfile`, not an explicit chmod. Malformed config is never overwritten. A legacy Mouser config is imported read-only. Unknown fields at any level survive migration and settings edits. Profile order follows the file because the workspace enables the serde_json `preserve_order` feature. The first matching profile wins, compared case-insensitively.
- macOS shortcuts use hardware virtual key codes (kVK), not ASCII. For example cmd is 55, shift is 56 and digit 3 is 20. Windows virtual key codes for letters equal uppercase ASCII and digits equal ASCII.
- On Windows, browser back and forward press left Alt (VK_LMENU 0xa4) and the arrow key one at a time, 10 ms apart.
- `Endpoint::wake` connects and drops the connection to unblock the service accept loop. `accept` fails on that empty connection and the loop keeps serving the next one.
- Save conflicts are detected with the service instance and the revision. The revision restarts at 1 on every service launch.
- `superlight_call` in the macOS bridge always returns a JSON object that the caller frees with `superlight_free`. `superlight_free(null)` does nothing.
- `compatibility/mice.json` and `superlight_core::devices::DEVICES` must change together. No automated check enforces this.
