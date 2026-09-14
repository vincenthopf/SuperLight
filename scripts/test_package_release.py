import contextlib
import hashlib
import io
import json
import os
import pathlib
import plistlib
import stat
import struct
import subprocess
import tempfile
import unittest
import zipfile
from unittest.mock import patch

import package_release as release


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="superlight-release-test-")
        self.addCleanup(temporary.cleanup)
        self.root = pathlib.Path(temporary.name).resolve()
        self.source = self.root / "binaries"
        self.source.mkdir()
        self.header = struct.pack("<8I", 0xFEEDFACF, 0x0100000C, 0, 2, 0, 0, 0, 0)
        for name in ("superlight", "superlight-ui"):
            (self.source / name).write_bytes(self.header)
        self.assets = self.root / "assets"
        self.assets.mkdir()
        for name in ("LICENSE", "README.md", "AppIcon.icns", "mouse.png", "mouse_mx_anywhere_3s.png", "mx_vertical.png"):
            (self.assets / name).write_bytes(name.encode())
        self.destination = self.root / "stage"
        self.output = self.root / "package.zip"
        self.version = "4.0.0-alpha.2"
        (self.root / "Cargo.toml").write_text('[workspace.package]\nversion = "4.0.0-alpha.2"\n')

    def stage(self):
        return release.stage(self.source, self.destination, self.assets, "Darwin", self.version)

    def test_macho_requires_complete_arm64_executable_header(self):
        executable = self.source / "superlight"
        self.assertEqual(release.executable_architecture(executable, "Darwin"), "arm64")
        invalid = [self.header[:length] for length in (0, 4, 8, 31)]
        invalid += [b"bad!" + self.header[4:]]
        invalid += [struct.pack("<8I", 0xFEEDFACF, 0x01000007, 0, 2, 0, 0, 0, 0)]
        invalid += [struct.pack("<8I", 0xFEEDFACF, 0x0100000C, 0, kind, 0, 0, 0, 0) for kind in (1, 6)]
        for header in invalid:
            with self.subTest(header=header):
                executable.write_bytes(header)
                with self.assertRaises(ValueError):
                    release.executable_architecture(executable, "Darwin")

    def test_existing_pe_architecture_detection_is_preserved(self):
        executable = self.source / "superlight.exe"
        for machine, architecture in ((0x8664, "x64"), (0xAA64, "arm64")):
            executable.write_bytes(b"MZ" + bytes(58) + struct.pack("<I", 64) + b"PE\0\0" + struct.pack("<H", machine))
            self.assertEqual(release.executable_architecture(executable, "Windows"), architecture)
        with self.assertRaises(ValueError):
            release.executable_architecture(executable, "Linux")

    def test_missing_required_inputs_do_not_create_stage(self):
        for path in (self.source / "superlight", self.assets / "LICENSE", self.assets / "AppIcon.icns", self.assets / "mouse.png"):
            with self.subTest(path=path.name):
                content = path.read_bytes()
                path.unlink()
                with self.assertRaises(FileNotFoundError):
                    self.stage()
                self.assertFalse(self.destination.exists())
                path.write_bytes(content)
        (self.source / "superlight").write_bytes(b"")
        with self.assertRaises(FileNotFoundError):
            self.stage()
        self.assertFalse(self.destination.exists())

    def test_binary_symlink_is_rejected_before_read_or_stage(self):
        binary = self.source / "superlight"
        binary.unlink()
        binary.symlink_to(self.source / "superlight-ui")
        with self.assertRaisesRegex(ValueError, "Symlinks"):
            release.executable_architecture(binary, "Darwin")
        with self.assertRaisesRegex(ValueError, "Symlinks"):
            self.stage()
        self.assertFalse(self.destination.exists())

    def test_asset_symlinks_are_rejected_before_stage(self):
        directory = self.root / "external"
        directory.mkdir()
        (directory / "LICENSE").write_text("outside fixture")
        for target in (directory, directory / "LICENSE", directory / "missing"):
            with self.subTest(target=target):
                link = self.assets / "linked"
                link.symlink_to(target)
                with self.assertRaisesRegex(ValueError, "Symlinks"):
                    self.stage()
                self.assertFalse(self.destination.exists())
                link.unlink()

    def test_symlinked_source_directory_is_rejected(self):
        link = self.root / "linked-binaries"
        link.symlink_to(self.source, target_is_directory=True)
        with self.assertRaisesRegex(ValueError, "Symlinks"):
            release.stage(link, self.destination, self.assets, "Darwin", self.version)
        self.assertFalse(self.destination.exists())

    def test_archive_contract_and_determinism(self):
        licenses = self.assets / "THIRD_PARTY_LICENSES" / "example-1.0.0"
        licenses.mkdir(parents=True)
        (licenses / "LICENSE").write_bytes(b"dependency license fixture")
        bundle = self.stage()
        information = plistlib.loads((bundle / "Contents/Info.plist").read_bytes())
        expected = {
            "CFBundleDevelopmentRegion": "en", "CFBundleExecutable": "superlight",
            "CFBundleIconFile": "AppIcon.icns", "CFBundleIdentifier": "io.github.vincenthopf.SuperLight",
            "CFBundleInfoDictionaryVersion": "6.0", "CFBundleName": "SuperLight",
            "CFBundleDisplayName": "SuperLight", "CFBundlePackageType": "APPL",
            "CFBundleShortVersionString": "4.0.0", "CFBundleVersion": "4.0.0",
            "LSMinimumSystemVersion": "14.0", "LSUIElement": True, "NSHighResolutionCapable": True,
            "NSInputMonitoringUsageDescription": "SuperLight reads mouse input to apply your configured button and gesture mappings.",
            "NSAccessibilityUsageDescription": "SuperLight sends only the mouse and keyboard actions you configure.",
            "SuperLightVersion": "4.0.0-alpha.2",
        }
        self.assertEqual(information, expected)
        release.archive(bundle, self.output)
        second = self.root / "second.zip"
        for path in release.package_files(bundle):
            os.utime(path, (1000000000, 1000000000))
        release.archive(bundle, second)
        self.assertEqual(self.output.read_bytes(), second.read_bytes())
        with zipfile.ZipFile(self.output) as archive:
            names = archive.namelist()
            self.assertEqual(names, sorted(names))
            expected_names = {
                "SuperLight.app/Contents/Info.plist",
                "SuperLight.app/Contents/MacOS/superlight",
                "SuperLight.app/Contents/MacOS/superlight-ui",
                "SuperLight.app/Contents/Resources/THIRD_PARTY_LICENSES/example-1.0.0/LICENSE",
            }
            expected_names.update("SuperLight.app/Contents/Resources/" + name for name in
                                  ("LICENSE", "README.md", "AppIcon.icns", "mouse.png", "mouse_mx_anywhere_3s.png", "mx_vertical.png"))
            self.assertEqual(set(names), expected_names)
            self.assertIsNone(archive.testzip())
            self.assertEqual(archive.read("SuperLight.app/Contents/Resources/THIRD_PARTY_LICENSES/example-1.0.0/LICENSE"), b"dependency license fixture")
            for entry in archive.infolist():
                self.assertEqual(entry.date_time, (2026, 1, 1, 0, 0, 0))
                self.assertEqual(entry.create_system, 3)
                self.assertEqual(entry.compress_type, zipfile.ZIP_DEFLATED)
                mode = 0o755 if "/MacOS/" in entry.filename else 0o644
                self.assertEqual(entry.external_attr >> 16, stat.S_IFREG | mode)
        for name in ("superlight", "superlight-ui"):
            self.assertEqual((bundle / "Contents/MacOS" / name).stat().st_mode & 0o777, 0o755)

    def test_archive_rejects_symlinks_without_creating_output(self):
        bundle = self.stage()
        (bundle / "late-link").symlink_to(self.assets / "LICENSE")
        with self.assertRaisesRegex(ValueError, "Symlinks"):
            release.archive(bundle, self.output)
        self.assertFalse(self.output.exists())

    def test_archive_rejects_non_regular_files_without_creating_output(self):
        bundle = self.stage()
        os.mkfifo(bundle / "pipe")
        with self.assertRaisesRegex(ValueError, "Unsupported release file type"):
            release.archive(bundle, self.output)
        self.assertFalse(self.output.exists())

    def test_existing_stage_and_archive_are_never_overwritten(self):
        bundle = self.stage()
        with self.assertRaises(FileExistsError):
            self.stage()
        self.output.write_bytes(b"existing archive fixture")
        with self.assertRaises(FileExistsError):
            release.archive(bundle, self.output)
        self.assertEqual(self.output.read_bytes(), b"existing archive fixture")

    def test_workspace_version_and_tag_validation(self):
        for environment in ({}, {"GITHUB_REF": "refs/heads/master"},
                            {"GITHUB_REF": "refs/tags/v4.0.0-alpha.2", "GITHUB_REF_NAME": "v4.0.0-alpha.2"}):
            self.assertEqual(release.workspace_version(self.root, environment), self.version)
        with self.assertRaisesRegex(ValueError, "Tag must match workspace version"):
            release.workspace_version(self.root, {"GITHUB_REF": "refs/tags/v4.0.0", "GITHUB_REF_NAME": "v4.0.0"})
        for version in ("bad", "4.0", "4.0.0/escape", "4.0.0\n"):
            with self.subTest(version=version), self.assertRaises(ValueError):
                release.bundle_version(version)
        self.assertEqual(release.bundle_version("4.0.0"), "4.0.0")

    def license_metadata(self, license_file=None):
        directory = self.root / "dependency"
        directory.mkdir(exist_ok=True)
        package = {"name": "example", "version": "1.0.0", "source": "registry+fixture",
                   "manifest_path": str(directory / "Cargo.toml"), "license": "MIT",
                   "license_file": license_file, "repository": "https://example.invalid"}
        return directory, {"packages": [package, {"name": "local", "version": "1.0.0", "source": None}]}

    def test_dependency_license_contents_index_and_checked_metadata(self):
        directory, metadata = self.license_metadata("legal/terms.txt")
        (directory / "LICENSE").write_bytes(b"root license fixture")
        (directory / "legal").mkdir()
        (directory / "legal/terms.txt").write_bytes(b"declared license fixture")
        (directory / "COPYING-large").write_bytes(bytes(2 * 1024 * 1024 + 1))
        destination = self.root / "licenses"
        with patch.object(release.subprocess, "run", return_value=subprocess.CompletedProcess([], 0, json.dumps(metadata))) as run:
            release.dependency_licenses(self.root, destination)
        run.assert_called_once_with(["cargo", "metadata", "--locked", "--format-version", "1"],
                                    cwd=self.root, text=True, capture_output=True, timeout=60, check=True)
        self.assertEqual(json.loads((destination / "index.json").read_text()), [
            {"name": "example", "version": "1.0.0", "license": "MIT", "repository": "https://example.invalid",
             "files": ["LICENSE", "legal/terms.txt"]}])
        self.assertEqual((destination / "example-1.0.0/legal/terms.txt").read_bytes(), b"declared license fixture")
        self.assertFalse((destination / "local-1.0.0").exists())

    def test_dependency_license_escape_and_symlink_are_rejected(self):
        outside = self.root / "outside"
        outside.mkdir()
        (outside / "terms.txt").write_text("outside fixture")
        directory, metadata = self.license_metadata("legal/terms.txt")
        (directory / "legal").symlink_to(outside, target_is_directory=True)
        for index, license_file in enumerate(("legal/terms.txt", "../outside/terms.txt", str(outside / "terms.txt"))):
            metadata["packages"][0]["license_file"] = license_file
            with self.subTest(license_file=license_file), patch.object(release.subprocess, "run", return_value=subprocess.CompletedProcess([], 0, json.dumps(metadata))):
                with self.assertRaises(ValueError):
                    release.dependency_licenses(self.root, self.root / f"licenses-{index}")
                self.assertEqual(list((self.root / f"licenses-{index}").rglob("terms.txt")), [])

    def invoke_main(self, *arguments, failure=None):
        stdout = io.StringIO()
        def run(command, **kwargs):
            if failure == command[0]:
                raise subprocess.CalledProcessError(1, command)
            return subprocess.CompletedProcess(command, 0, json.dumps({"packages": []}))
        with patch.object(release, "__file__", str(self.root / "scripts/package_release.py")), \
             patch("sys.argv", ["package_release.py", "--binaries", str(self.source), "--output", str(self.root / "dist"), *arguments]), \
             patch.object(release.platform, "system", return_value="Darwin"), \
             patch.dict(os.environ, {}, clear=True), \
             patch.object(release.subprocess, "run", side_effect=run) as commands, \
             contextlib.redirect_stdout(stdout):
            release.main()
        return stdout.getvalue(), commands.call_args_list

    def prepare_main_assets(self):
        (self.root / "LICENSE").write_text("license fixture")
        (self.root / "README.md").write_text("readme fixture")
        images = self.root / "images"
        images.mkdir()
        for name in ("AppIcon.icns", "mouse.png", "mouse_mx_anywhere_3s.png", "mx_vertical.png"):
            (images / name).write_bytes(name.encode())

    def test_version_check_needs_neither_binaries_nor_subprocesses(self):
        output, commands = self.invoke_main("--check-version", "--binaries", str(self.root / "missing"))
        self.assertEqual(output, "4.0.0-alpha.2\n")
        self.assertEqual(commands, [])
        self.assertFalse((self.root / "dist").exists())

    def test_main_checksum_signing_commands_and_no_overwrite(self):
        self.prepare_main_assets()
        output, commands = self.invoke_main("--architecture", "arm64")
        receipt = json.loads(output)
        archive = pathlib.Path(receipt["archive"])
        self.assertEqual(archive.name, "SuperLight-4.0.0-alpha.2-macOS-arm64.zip")
        digest = hashlib.sha256(archive.read_bytes()).hexdigest()
        self.assertEqual(receipt["sha256"], digest)
        self.assertEqual(receipt["signing"], "ad-hoc, not notarized")
        self.assertEqual(archive.with_suffix(".zip.sha256").read_bytes(), f"{digest}  {archive.name}\n".encode())
        signing = [call for call in commands if call.args[0][0] == "codesign"]
        self.assertEqual(len(signing), 4)
        self.assertEqual([pathlib.Path(call.args[0][-1]).name for call in signing], ["superlight-ui", "superlight", "SuperLight.app", "SuperLight.app"])
        for call in signing[:3]:
            self.assertEqual(call.args[0][1:-1], ["--force", "--sign", "-", "--timestamp=none"])
        self.assertEqual(signing[-1].args[0][1:-1], ["--verify", "--deep", "--strict", "--verbose=2"])
        for call in signing:
            self.assertEqual(call.kwargs, {"check": True, "timeout": 60})
        with self.assertRaises(FileExistsError):
            self.invoke_main()
        self.assertEqual(hashlib.sha256(archive.read_bytes()).hexdigest(), digest)

    def test_main_subprocess_failures_do_not_create_artifacts(self):
        self.prepare_main_assets()
        for command in ("cargo", "codesign"):
            with self.subTest(command=command), self.assertRaises(subprocess.CalledProcessError):
                self.invoke_main(failure=command)
            self.assertEqual(list((self.root / "dist").iterdir()), [])

    def test_main_rejects_original_asset_symlink_before_staging(self):
        self.prepare_main_assets()
        (self.root / "LICENSE").unlink()
        (self.root / "LICENSE").symlink_to(self.assets / "LICENSE")
        with self.assertRaisesRegex(ValueError, "Symlinks"):
            self.invoke_main()
        self.assertEqual(list((self.root / "dist").iterdir()), [])

    def test_main_rejects_mismatched_expected_architecture(self):
        with self.assertRaisesRegex(SystemExit, "Expected x64 executables, found arm64"):
            self.invoke_main("--architecture", "x64")
        self.assertFalse((self.root / "dist").exists())

    def test_main_refuses_existing_or_dangling_checksum(self):
        output = self.root / "dist"
        output.mkdir()
        checksum = output / "SuperLight-4.0.0-alpha.2-macOS-arm64.zip.sha256"
        checksum.write_bytes(b"existing checksum fixture")
        with self.assertRaises(FileExistsError):
            self.invoke_main()
        self.assertEqual(checksum.read_bytes(), b"existing checksum fixture")
        checksum.unlink()
        checksum.symlink_to(self.root / "missing")
        with self.assertRaises(FileExistsError):
            self.invoke_main()
        self.assertTrue(checksum.is_symlink())
        self.assertEqual(len(list(output.iterdir())), 1)


if __name__ == "__main__":
    unittest.main()
