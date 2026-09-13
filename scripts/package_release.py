import argparse
import hashlib
import json
import os
import pathlib
import platform
import plistlib
import re
import shutil
import stat
import subprocess
import tempfile
import tomllib
import zipfile


def stage(source, destination, assets, system, version):
    if system not in ("Darwin", "Windows"):
        raise ValueError(f"Unsupported platform: {system}")
    match = re.fullmatch(r"(\d+\.\d+\.\d+)(?:-[A-Za-z0-9.-]+)?", version)
    if match is None:
        raise ValueError("Invalid application version")
    suffix = ".exe" if system == "Windows" else ""
    binaries = [source / (name + suffix) for name in ("superlight", "superlight-ui")]
    for binary in binaries:
        if not binary.is_file() or binary.stat().st_size == 0:
            raise FileNotFoundError(f"Missing native executable: {binary}")
    if not (assets / "LICENSE").is_file():
        raise FileNotFoundError("The upstream license must be included")
    if system == "Darwin" and not (assets / "AppIcon.icns").is_file():
        raise FileNotFoundError("The macOS application icon is missing")
    if system == "Darwin":
        for image in ("mouse.png", "mouse_mx_anywhere_3s.png", "mx_vertical.png"):
            if not (assets / image).is_file():
                raise FileNotFoundError(f"Missing mouse artwork: {image}")
    destination.mkdir(parents=True, exist_ok=False)
    bundle = destination / ("SuperLight.app" if system == "Darwin" else "SuperLight")
    executable_directory = bundle / "Contents/MacOS" if system == "Darwin" else bundle
    resources = bundle / "Contents/Resources" if system == "Darwin" else bundle
    executable_directory.mkdir(parents=True)
    resources.mkdir(parents=True, exist_ok=True)
    for binary in binaries:
        target = executable_directory / binary.name
        shutil.copyfile(binary, target)
        target.chmod(0o755)
    for asset in sorted(assets.iterdir()):
        if asset.is_file():
            shutil.copyfile(asset, resources / asset.name)
        elif asset.is_dir():
            shutil.copytree(asset, resources / asset.name)
    if system == "Darwin":
        information = {
            "CFBundleDevelopmentRegion": "en",
            "CFBundleExecutable": "superlight",
            "CFBundleIconFile": "AppIcon.icns",
            "CFBundleIdentifier": "io.github.vincenthopf.SuperLight",
            "CFBundleInfoDictionaryVersion": "6.0",
            "CFBundleName": "SuperLight",
            "CFBundleDisplayName": "SuperLight",
            "CFBundlePackageType": "APPL",
            "CFBundleShortVersionString": match.group(1),
            "CFBundleVersion": match.group(1),
            "LSMinimumSystemVersion": "26.0",
            "LSUIElement": True,
            "NSHighResolutionCapable": True,
            "NSInputMonitoringUsageDescription": "SuperLight reads mouse input to apply your configured button and gesture mappings.",
            "NSAccessibilityUsageDescription": "SuperLight sends only the mouse and keyboard actions you configure.",
            "SuperLightVersion": version,
        }
        (bundle / "Contents/Info.plist").write_bytes(plistlib.dumps(information, sort_keys=True))
    return bundle


def archive(bundle, output):
    with zipfile.ZipFile(output, "x", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as result:
        for path in sorted(bundle.rglob("*")):
            if path.is_symlink():
                raise ValueError(f"Symlinks are not permitted in the release package: {path}")
            if not path.is_file():
                continue
            entry = zipfile.ZipInfo(path.relative_to(bundle.parent).as_posix(), (2026, 1, 1, 0, 0, 0))
            entry.create_system = 3
            executable = path.name in ("superlight", "superlight-ui", "superlight.exe", "superlight-ui.exe", "install-permissions.sh")
            entry.external_attr = (stat.S_IFREG | (0o755 if executable else 0o644)) << 16
            entry.compress_type = zipfile.ZIP_DEFLATED
            result.writestr(entry, path.read_bytes(), compresslevel=9)


def dependency_licenses(root, destination):
    result = subprocess.run(
        ["cargo", "metadata", "--locked", "--format-version", "1"],
        cwd=root, text=True, capture_output=True, timeout=60, check=True,
    )
    metadata = json.loads(result.stdout)
    destination.mkdir()
    index = []
    for package in sorted(metadata["packages"], key=lambda package: (package["name"], package["version"])):
        if package.get("source") is None:
            continue
        directory = pathlib.Path(package["manifest_path"]).parent
        name = f'{package["name"]}-{package["version"]}'
        target = destination / name
        target.mkdir()
        candidates = set()
        for pattern in ("*LICENSE*", "*LICENCE*", "*COPYING*", "*NOTICE*", "*COPYRIGHT*", "*license*", "*licence*"):
            candidates.update(directory.glob(pattern))
            candidates.update(directory.glob("*/" + pattern))
        if package.get("license_file"):
            candidates.add(directory / package["license_file"])
        copied = []
        for path in sorted(candidates):
            if not path.is_file() or path.stat().st_size > 2 * 1024 * 1024:
                continue
            relative = path.relative_to(directory)
            if ".." in relative.parts:
                raise ValueError(f"A dependency license escaped its package: {name}")
            output = target / relative
            output.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(path, output)
            copied.append(str(relative))
        index.append({"name": package["name"], "version": package["version"], "license": package.get("license"), "repository": package.get("repository"), "files": copied})
    (destination / "index.json").write_text(json.dumps(index, indent=2) + "\n", encoding="utf-8")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binaries", type=pathlib.Path, default=pathlib.Path("target/release"))
    parser.add_argument("--output", type=pathlib.Path, default=pathlib.Path("dist"))
    arguments = parser.parse_args()
    root = pathlib.Path(__file__).resolve().parents[1]
    system = platform.system()
    if system not in ("Darwin", "Windows"):
        raise SystemExit("Release packages support macOS and Windows only")
    architecture = {"aarch64": "arm64", "arm64": "arm64", "x86_64": "x64", "amd64": "x64"}.get(platform.machine().lower())
    if architecture is None:
        raise SystemExit("The current CPU architecture is not supported by release packaging")
    version = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))["workspace"]["package"]["version"]
    target = arguments.output.resolve()
    target.mkdir(parents=True, exist_ok=True)
    label = {"Darwin": "macOS", "Windows": "Windows", "Linux": "Linux"}[system]
    output = target / f"SuperLight-{version}-{label}-{architecture}.zip"
    if output.exists() or output.with_suffix(".zip.sha256").exists():
        raise FileExistsError(f"The release artifact already exists: {output}")
    with tempfile.TemporaryDirectory(prefix="superlight-package-") as temporary:
        temporary = pathlib.Path(temporary)
        assets = temporary / "assets"
        assets.mkdir()
        shutil.copyfile(root / "LICENSE", assets / "LICENSE")
        for filename in ("README.md", "docs/RUST_REWRITE.md"):
            path = root / filename
            if path.exists():
                shutil.copyfile(path, assets / path.name)
        if system == "Darwin":
            for image in ("AppIcon.icns", "mouse.png", "mouse_mx_anywhere_3s.png", "mx_vertical.png"):
                shutil.copyfile(root / "images" / image, assets / image)
        dependency_licenses(root, assets / "THIRD_PARTY_LICENSES")
        bundle = stage(arguments.binaries.resolve(), temporary / "stage", assets, system, version)
        if system == "Darwin":
            for path in (bundle / "Contents/MacOS/superlight-ui", bundle / "Contents/MacOS/superlight", bundle):
                subprocess.run(["codesign", "--force", "--sign", "-", "--timestamp=none", str(path)], check=True, timeout=60)
            subprocess.run(["codesign", "--verify", "--deep", "--strict", "--verbose=2", str(bundle)], check=True, timeout=60)
        archive(bundle, output)
    digest = hashlib.sha256(output.read_bytes()).hexdigest()
    with output.with_suffix(".zip.sha256").open("x", encoding="utf-8") as checksum:
        checksum.write(f"{digest}  {output.name}\n")
    print(json.dumps({"archive": str(output), "sha256": digest, "bytes": output.stat().st_size, "signing": "ad-hoc, not notarized" if system == "Darwin" else "unsigned", "version": version, "architecture": architecture}))


if __name__ == "__main__":
    main()
