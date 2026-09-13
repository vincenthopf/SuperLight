set -eu
root=$(cd "$(dirname "$0")/../.." && pwd)
work="$root/.working/inspector"
out="$work/SuperLight.app"
mkdir -p "$out/Contents/MacOS" "$out/Contents/Resources" "$work"
cargo build -p superlight-macos-bridge --release --target aarch64-apple-darwin
xcrun swiftc -parse-as-library "$root"/macos/inspector/*.swift -import-objc-header "$root/macos/inspector/Bridge.h" -I "$root/target/aarch64-apple-darwin/release" -L "$root/target/aarch64-apple-darwin/release" -lsuperlight_macos_bridge -o "$out/Contents/MacOS/SuperLight" -framework SwiftUI -framework AppKit -target arm64-apple-macosx26.0
cp "$root"/images/mouse.png "$out/Contents/Resources/mouse.png"
cp "$root"/images/mouse_mx_anywhere_3s.png "$out/Contents/Resources/mouse_mx_anywhere_3s.png"
cp "$root"/images/mx_vertical.png "$out/Contents/Resources/mx_vertical.png"
cat > "$out/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>CFBundleExecutable</key><string>SuperLight</string><key>CFBundleIdentifier</key><string>io.github.vincenthopf.SuperLight</string><key>CFBundleName</key><string>SuperLight</string><key>CFBundlePackageType</key><string>APPL</string><key>LSMinimumSystemVersion</key><string>13.0</string><key>NSHighResolutionCapable</key><true/><key>NSAccessibilityUsageDescription</key><string>SuperLight sends the actions you configure for your Logitech mouse.</string><key>NSInputMonitoringUsageDescription</key><string>SuperLight reads Logitech mouse input to apply your mappings.</string></dict></plist>
PLIST
codesign --force --sign - "$out"
printf '%s\n' "$out"
