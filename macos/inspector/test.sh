set -eu
root=$(cd "$(dirname "$0")/../.." && pwd)
cd "$root"
work="$root/.working/inspector/test-$(date +%Y%m%d-%H%M%S)-$$"
mkdir -p "$work"
case "$(uname -m)" in
  arm64) rust_target=aarch64-apple-darwin; swift_target=arm64-apple-macosx14.0 ;;
  x86_64) rust_target=x86_64-apple-darwin; swift_target=x86_64-apple-macosx14.0 ;;
esac
xcrun swiftc -parse-as-library -warnings-as-errors macos/inspector/ServiceModel.swift macos/inspector/tests/Integration.swift -import-objc-header macos/inspector/Bridge.h "${CARGO_TARGET_DIR:-$root/target}/$rust_target/release/libsuperlight_macos_bridge.a" -o "$work/integration-tests" -framework SwiftUI -framework AppKit -target "$swift_target"
export SUPERLIGHT_CONFIG_DIR="$work/config"
target/native-macos/superlight --headless >"$work/service.log" 2>&1 &
service=$!
trap 'target/native-macos/superlight --quit >/dev/null 2>&1 || true; wait "$service" || true' EXIT
for attempt in $(seq 1 30); do
  if target/native-macos/superlight --status >"$work/status.json" 2>/dev/null; then break; fi
  sleep 0.1
done
"$work/integration-tests"
