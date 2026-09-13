set -eu
root=$(cd "$(dirname "$0")/../.." && pwd)
cd "$root"
if [ -z "${DEVELOPER_DIR:-}" ]; then
    for xcode in /Applications/Xcode_26*.app; do
        if [ -d "$xcode" ]; then export DEVELOPER_DIR="$xcode/Contents/Developer"; break; fi
    done
fi
if ! xcodebuild -version | head -1 | grep -qE "Xcode (26|[3-9][0-9])"; then
    printf "Xcode 26 or newer is required. Select it with xcode-select.\n" >&2
    exit 1
fi
out="${1:-$root/target/native-macos}"
target_dir="${CARGO_TARGET_DIR:-$root/target}"
case "$(uname -m)" in
  arm64) rust_target=aarch64-apple-darwin; swift_target=arm64-apple-macosx14.0 ;;
  *) printf 'Unsupported architecture\n' >&2; exit 1 ;;
esac
mkdir -p "$out"
cargo build --locked --release --target "$rust_target" -p superlight-macos-bridge -p superlight-service --bins --lib
xcrun swiftc -parse-as-library -warnings-as-errors "$root"/macos/inspector/*.swift -import-objc-header "$root/macos/inspector/Bridge.h" "$target_dir/$rust_target/release/libsuperlight_macos_bridge.a" -o "$out/superlight-ui" -framework SwiftUI -framework AppKit -target "$swift_target"
cp "$target_dir/$rust_target/release/superlight" "$out/superlight"
printf '%s\n' "$out"
