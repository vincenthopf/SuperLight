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


def reject_symlinks(path, root):
    relative = path.relative_to(root)
    if ".." in relative.parts:
        raise ValueError(f"A release source escaped its directory: {path}")
    current = root
    for part in ("", *relative.parts):
        current = current / part
        if current.is_symlink():
            raise ValueError(f"Symlinks are not permitted in release sources: {current}")


def package_files(directory):
    if directory.is_symlink() or not directory.is_dir():
        raise ValueError(f"Expected a release directory, not a symlink: {directory}")
    files = []
    for path in sorted(directory.iterdir()):
        if path.is_symlink():
            raise ValueError(f"Symlinks are not permitted in the release package: {path}")
        if path.is_dir():
            files.extend(package_files(path))
        elif path.is_file():
            files.append(path)
        else:
            raise ValueError(f"Unsupported release file type: {path}")
    return sorted(files)


def bundle_version(version):
    match = re.fullmatch(r"(\d+\.\d+\.\d+)(?:-[A-Za-z0-9.-]+)?", version)
    if match is None:
        raise ValueError("Invalid application version")
    return match.group(1)


def workspace_version(root, environment):
    version = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))["workspace"]["package"]["version"]
    bundle_version(version)
    if environment.get("GITHUB_REF", "").startswith("refs/tags/") and environment.get("GITHUB_REF_NAME") != "v" + version:
        raise ValueError("Tag must match workspace version: v" + version)
    return version


def executable_architecture(path, system):
    if system not in ("Darwin", "Windows"):
        raise ValueError(f"Unsupported platform: {system}")
    reject_symlinks(path, path.parent)
    if not path.is_file():
        raise ValueError(f"Expected a native executable file: {path}")
    with path.open("rb") as binary:
        header = binary.read(64)
        if system == "Darwin":
            if len(header) < 32 or header[:4] != b"\xcf\xfa\xed\xfe" or int.from_bytes(header[12:16], "little") != 2:
                raise ValueError(f"Expected a native Mach-O executable: {path}")
            machine = int.from_bytes(header[4:8], "little")
            architecture = {0x0100000C: "arm64"}.get(machine)
        else:
            if header[:2] != b"MZ" or len(header) < 64:
                raise ValueError(f"Expected a Windows executable: {path}")
            binary.seek(int.from_bytes(header[60:64], "little"))
            pe = binary.read(6)
            if pe[:4] != b"PE\0\0":
                raise ValueError(f"Invalid Windows executable: {path}")
            architecture = {0x8664: "x64", 0xAA64: "arm64"}.get(int.from_bytes(pe[4:6], "little"))
    if architecture is None:
        raise ValueError(f"Unsupported executable architecture: {path}")
    return architecture


def stage(source, destination, assets, system, version):
    if system not in ("Darwin", "Windows"):
        raise ValueError(f"Unsupported platform: {system}")
    short_version = bundle_version(version)
    suffix = ".exe" if system == "Windows" else ""
    binaries = [source / (name + suffix) for name in ("superlight", "superlight-ui")]
    for binary in binaries:
        reject_symlinks(binary, source)
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
    assets_to_copy = package_files(assets)
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
    for asset in assets_to_copy:
        target = resources / asset.relative_to(assets)
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(asset, target)
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
            "CFBundleShortVersionString": short_version,
            "CFBundleVersion": short_version,
            "LSMinimumSystemVersion": "14.0",
            "LSUIElement": True,
            "NSHighResolutionCapable": True,
            "NSInputMonitoringUsageDescription": "SuperLight reads mouse input to apply your configured button and gesture mappings.",
            "NSAccessibilityUsageDescription": "SuperLight sends only the mouse and keyboard actions you configure.",
            "SuperLightVersion": version,
        }
        (bundle / "Contents/Info.plist").write_bytes(plistlib.dumps(information, sort_keys=True))
    return bundle


def archive(bundle, output):
    files = package_files(bundle)
    with zipfile.ZipFile(output, "x", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as result:
        for path in files:
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
            reject_symlinks(path, directory)
            if not path.is_file() or path.stat().st_size > 2 * 1024 * 1024:
                continue
            relative = path.relative_to(directory)
            output = target / relative
            output.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(path, output)
            copied.append(str(relative))
        index.append({"name": package["name"], "version": package["version"], "license": package.get("license"), "repository": package.get("repository"), "files": copied})
    (destination / "index.json").write_text(json.dumps(index, indent=2) + "\n", encoding="utf-8")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binaries", type=pathlib.Path, default=pathlib.Path("target/release"))
    parser.add_argument("--architecture", choices=["arm64", "x64"])
    parser.add_argument("--output", type=pathlib.Path, default=pathlib.Path("dist"))
    parser.add_argument("--check-version", action="store_true")
    arguments = parser.parse_args()
    root = pathlib.Path(__file__).resolve().parents[1]
    version = workspace_version(root, os.environ)
    if arguments.check_version:
        print(version)
        return
    system = platform.system()
    if system not in ("Darwin", "Windows"):
        raise SystemExit("Release packages support macOS and Windows only")
    suffix = ".exe" if system == "Windows" else ""
    architectures = {executable_architecture(arguments.binaries / (name + suffix), system) for name in ("superlight", "superlight-ui")}
    if len(architectures) != 1:
        raise SystemExit("Service and settings executables have different architectures")
    architecture = architectures.pop()
    if arguments.architecture and architecture != arguments.architecture:
        raise SystemExit(f"Expected {arguments.architecture} executables, found {architecture}")
    target = arguments.output.resolve()
    target.mkdir(parents=True, exist_ok=True)
    label = {"Darwin": "macOS", "Windows": "Windows"}[system]
    output = target / f"SuperLight-{version}-{label}-{architecture}.zip"
    if any(path.exists() or path.is_symlink() for path in (output, output.with_suffix(".zip.sha256"))):
        raise FileExistsError(f"The release artifact already exists: {output}")
    with tempfile.TemporaryDirectory(prefix="superlight-package-") as temporary:
        temporary = pathlib.Path(temporary)
        assets = temporary / "assets"
        assets.mkdir()
        for name in ("LICENSE", "README.md"):
            reject_symlinks(root / name, root)
            shutil.copyfile(root / name, assets / name)
        if system == "Darwin":
            for image in ("AppIcon.icns", "mouse.png", "mouse_mx_anywhere_3s.png", "mx_vertical.png"):
                reject_symlinks(root / "images" / image, root)
                shutil.copyfile(root / "images" / image, assets / image)
        dependency_licenses(root, assets / "THIRD_PARTY_LICENSES")
        bundle = stage(arguments.binaries.absolute(), temporary / "stage", assets, system, version)
        if system == "Darwin":
            for path in (bundle / "Contents/MacOS/superlight-ui", bundle / "Contents/MacOS/superlight", bundle):
                subprocess.run(["codesign", "--force", "--sign", "-", "--timestamp=none", str(path)], check=True, timeout=60)
            subprocess.run(["codesign", "--verify", "--deep", "--strict", "--verbose=2", str(bundle)], check=True, timeout=60)
        archive(bundle, output)
    digest = hashlib.sha256(output.read_bytes()).hexdigest()
    with output.with_suffix(".zip.sha256").open("x", encoding="utf-8", newline="") as checksum:
        checksum.write(f"{digest}  {output.name}\n")
    print(json.dumps({"archive": str(output), "sha256": digest, "bytes": output.stat().st_size, "signing": "ad-hoc, not notarized" if system == "Darwin" else "unsigned", "version": version, "architecture": architecture}))


if __name__ == "__main__":
    main()
