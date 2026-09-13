set -eu
root=$(cd "$(dirname "$0")/../.." && pwd)
bash "$root/macos/inspector/build.sh" >/dev/null
stamp=$(date +%Y%m%d-%H%M%S)
app="$root/.working/inspector/SuperLight-$stamp.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$root/target/native-macos/superlight" "$app/Contents/MacOS/superlight"
cp "$root/target/native-macos/superlight-ui" "$app/Contents/MacOS/superlight-ui"
for image in mouse.png mouse_mx_anywhere_3s.png mx_vertical.png AppIcon.icns; do cp "$root/images/$image" "$app/Contents/Resources/$image"; done
cat > "$app/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>CFBundleExecutable</key><string>superlight-ui</string><key>CFBundleIdentifier</key><string>io.github.vincenthopf.SuperLight</string><key>CFBundleName</key><string>SuperLight</string><key>CFBundleIconFile</key><string>AppIcon.icns</string><key>CFBundlePackageType</key><string>APPL</string><key>LSMinimumSystemVersion</key><string>26.0</string><key>NSHighResolutionCapable</key><true/><key>NSAccessibilityUsageDescription</key><string>SuperLight sends the actions you configure.</string><key>NSInputMonitoringUsageDescription</key><string>SuperLight reads mouse input to apply your mappings.</string></dict></plist>
PLIST
for path in "$app/Contents/MacOS/superlight" "$app/Contents/MacOS/superlight-ui" "$app"; do codesign --force --sign - "$path" >/dev/null; done
open "$app"
printf '%s\n' "$app"
