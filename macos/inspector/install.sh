set -eu
root=$(cd "$(dirname "$0")/../.." && pwd)
destination="$HOME/Applications/SuperLight.app"
if [ -e "$destination" ]; then
    printf 'SuperLight is already installed at %s. Keep the current installation running. Build a candidate with build.sh, then review and replace the installed app in Finder when ready.\n' "$destination" >&2
    exit 1
fi
mkdir -p "$root/.working/inspector"
bash "$root/macos/inspector/run.sh" --stage-only >"$root/.working/inspector/install-build.log"
source=$(tail -n 1 "$root/.working/inspector/install-build.log")
mkdir -p "$HOME/Applications"
ditto "$source" "$destination"
codesign --verify --deep --strict "$destination"
open "$destination"
printf '%s\n' "$destination"
