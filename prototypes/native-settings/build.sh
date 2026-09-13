set -eu
root=$(cd "$(dirname "$0")/../.." && pwd)
out="$root/.working/native-settings/build-$(date +%Y%m%d-%H%M%S)"
app="$out/SuperLight Designs.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
xcrun swiftc -parse-as-library "$root"/prototypes/native-settings/*.swift -o "$app/Contents/MacOS/SuperLightDesigns" -framework SwiftUI -framework AppKit -target arm64-apple-macosx26.0
cp "$root/images/mouse.png" "$app/Contents/Resources/mouse.png"
cp "$root/images/AppIcon.icns" "$app/Contents/Resources/AppIcon.icns"
cat > "$app/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>SuperLightDesigns</string>
<key>CFBundleIdentifier</key><string>com.superlight.native-design-review</string>
<key>CFBundleName</key><string>SuperLight Designs</string>
<key>CFBundleIconFile</key><string>AppIcon</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>LSMinimumSystemVersion</key><string>26.0</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
codesign --force --sign - "$app"
printf '%s\n' "$app"
