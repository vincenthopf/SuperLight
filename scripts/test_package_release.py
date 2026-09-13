import importlib.util
import os
import pathlib
import plistlib
import stat
import tempfile
import unittest
import zipfile

SPEC = importlib.util.spec_from_file_location("package_release", pathlib.Path(__file__).with_name("package_release.py"))
PACKAGE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PACKAGE)


class PackagingTests(unittest.TestCase):
    def fixture(self, root, system):
        source = root / "binaries"
        source.mkdir()
        for name in ("superlight", "superlight-ui"):
            (source / (name + (".exe" if system == "Windows" else ""))).write_bytes(b"native executable")
        assets = root / "assets"
        assets.mkdir()
        (assets / "LICENSE").write_text("MIT attribution", encoding="utf-8")
        (assets / "AppIcon.icns").write_bytes(b"icns")
        return source, assets

    def test_macos_bundle_contains_only_native_executables_and_resources(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            source, assets = self.fixture(root, "Darwin")
            bundle = PACKAGE.stage(source, root / "stage", assets, "Darwin", "4.0.0-alpha.1")
            self.assertEqual(bundle.name, "SuperLight.app")
            info = plistlib.loads((bundle / "Contents/Info.plist").read_bytes())
            self.assertEqual(info["CFBundleExecutable"], "superlight")
            self.assertEqual(info["CFBundleShortVersionString"], "4.0.0")
            self.assertTrue(info["LSUIElement"])
            executable = bundle / "Contents/MacOS/superlight"
            if os.name != "nt":
                self.assertTrue(executable.stat().st_mode & stat.S_IXUSR)
            self.assertEqual(sorted(path.name for path in executable.parent.iterdir()), ["superlight", "superlight-ui"])
            self.assertTrue((bundle / "Contents/Resources/LICENSE").exists())

    def test_windows_archive_keeps_service_and_settings_siblings(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            source, assets = self.fixture(root, "Windows")
            bundle = PACKAGE.stage(source, root / "stage", assets, "Windows", "4.0.0-alpha.1")
            output = root / "release.zip"
            PACKAGE.archive(bundle, output)
            with zipfile.ZipFile(output) as archive:
                names = set(archive.namelist())
                self.assertIn("SuperLight/superlight.exe", names)
                self.assertIn("SuperLight/superlight-ui.exe", names)
                self.assertIn("SuperLight/LICENSE", names)
                self.assertFalse(any(name.endswith(".py") for name in names))

    def test_existing_archives_and_staging_paths_are_not_overwritten(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            source, assets = self.fixture(root, "Linux")
            output = root / "release.zip"
            output.write_bytes(b"existing artifact")
            bundle = PACKAGE.stage(source, root / "stage", assets, "Linux", "4.0.0-alpha.1")
            with self.assertRaises(FileExistsError):
                PACKAGE.archive(bundle, output)
            self.assertEqual(output.read_bytes(), b"existing artifact")
            with self.assertRaises(FileExistsError):
                PACKAGE.stage(source, root / "stage", assets, "Linux", "4.0.0-alpha.1")

    def test_missing_executable_fails_before_creating_bundle(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            source, assets = self.fixture(root, "Linux")
            (source / "superlight-ui").unlink()
            with self.assertRaises(FileNotFoundError):
                PACKAGE.stage(source, root / "stage", assets, "Linux", "4.0.0-alpha.1")
            self.assertFalse((root / "stage").exists())

    def test_unix_zip_records_executable_permission_bits(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            source, assets = self.fixture(root, "Linux")
            bundle = PACKAGE.stage(source, root / "stage", assets, "Linux", "4.0.0-alpha.1")
            output = root / "release.zip"
            PACKAGE.archive(bundle, output)
            with zipfile.ZipFile(output) as archive:
                executable = archive.getinfo("SuperLight/superlight")
                self.assertTrue((executable.external_attr >> 16) & stat.S_IXUSR)


if __name__ == "__main__":
    unittest.main()
