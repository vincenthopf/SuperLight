import argparse
import contextlib
import json
import os
import pathlib
import subprocess
import tempfile
import time


def run(binary, environment, *arguments, ok=True):
    result = subprocess.run(
        [str(binary), *arguments], env=environment, text=True,
        capture_output=True, timeout=10,
    )
    if ok and result.returncode != 0:
        raise AssertionError(f"{arguments}: {result.returncode}\n{result.stdout}\n{result.stderr}")
    if not ok and result.returncode == 0:
        raise AssertionError(f"{arguments}: unexpectedly succeeded")
    return result


def snapshot(binary, environment):
    response = json.loads(run(binary, environment, "--status").stdout)
    assert response["ok"] and response["protocol"] == 1
    return response["snapshot"]


@contextlib.contextmanager
def service(binary, root):
    root.mkdir(mode=0o700, parents=True, exist_ok=True)
    environment = dict(os.environ, SUPERLIGHT_CONFIG_DIR=str(root))
    error = root / "service-error.txt"
    with (root / "service-output.txt").open("a") as stdout, error.open("a") as stderr:
        process = subprocess.Popen(
            [str(binary), "--headless", "--background"],
            env=environment, stdout=stdout, stderr=stderr,
        )
        try:
            deadline = time.monotonic() + 15
            while time.monotonic() < deadline:
                if process.poll() is not None:
                    raise AssertionError(f"Service exited with {process.returncode}: {error.read_text()}")
                state = subprocess.run(
                    [str(binary), "--status"], env=environment, text=True,
                    capture_output=True, timeout=5,
                )
                if state.returncode == 0:
                    break
                time.sleep(0.1)
            else:
                raise AssertionError("Service did not publish its local endpoint before the startup deadline")
            yield process, environment, json.loads(state.stdout)["snapshot"]
        finally:
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=5)
            if process.returncode not in (0, None):
                print(error.read_text(encoding="utf-8", errors="replace"))


def stop(binary, root, process, environment):
    run(binary, environment, "--quit")
    assert process.wait(timeout=10) == 0
    assert not (root / "run" / "endpoint.json").exists()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True, type=pathlib.Path)
    arguments = parser.parse_args()
    binary = arguments.binary.resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix="superlight-verification-") as temporary:
        root = pathlib.Path(temporary) / "main"
        with service(binary, root) as (process, environment, initial):
            assert initial["device"] is None
            assert not initial["native_ready"]
            assert initial["config"]["version"] == 9
            first_instance = initial["instance"]
            first_revision = initial["revision"]
            run(binary, environment, "--headless", "--background")
            assert process.poll() is None
            assert snapshot(binary, environment)["instance"] == first_instance
            run(binary, environment, "--pause")
            assert snapshot(binary, environment)["paused"]
            run(binary, environment, "--resume")
            assert not snapshot(binary, environment)["paused"]
            value = initial["config"]
            value["settings"]["dpi"] = 1600
            value["future_extension"] = {"must_survive": [1, 2, 3]}
            value["profiles"]["browser"] = {
                "label": "Browser", "apps": ["test.browser"],
                "mappings": {"middle": "copy"},
            }
            source = root / "apply.json"
            source.write_text(json.dumps(value), encoding="utf-8")
            response = json.loads(run(binary, environment, "--apply", str(source)).stdout)
            assert response["ok"]
            applied = response["snapshot"]
            assert applied["revision"] > first_revision
            assert applied["config"]["settings"]["dpi"] == 1600
            assert applied["config"]["future_extension"] == {"must_survive": [1, 2, 3]}
            saved = (root / "config.json").read_bytes()
            source.write_text("{invalid", encoding="utf-8")
            run(binary, environment, "--apply", str(source), ok=False)
            assert (root / "config.json").read_bytes() == saved
            value["profiles"]["default"]["mappings"]["middle"] = "custom:unknown_key"
            source.write_text(json.dumps(value), encoding="utf-8")
            run(binary, environment, "--apply", str(source), ok=False)
            assert (root / "config.json").read_bytes() == saved
            source.write_bytes(b" " * (1024 * 1024 + 1))
            run(binary, environment, "--apply", str(source), ok=False)
            assert (root / "config.json").read_bytes() == saved
            stop(binary, root, process, environment)
        with service(binary, root) as (process, environment, restarted):
            assert restarted["instance"] != first_instance
            assert restarted["config"]["settings"]["dpi"] == 1600
            assert restarted["config"]["profiles"]["browser"]["mappings"]["middle"] == "copy"
            assert restarted["config"]["future_extension"] == {"must_survive": [1, 2, 3]}
            assert (root / "config.json").read_bytes() == saved
            stop(binary, root, process, environment)
        migrated_root = pathlib.Path(temporary) / "migration"
        migrated_root.mkdir(mode=0o700)
        original = json.dumps({
            "version": 1,
            "settings": {"dpi": 1800, "start_with_windows": False},
            "profiles": {"default": {"mappings": {"middle": "copy"}}},
        }).encode()
        (migrated_root / "config.json").write_bytes(original)
        with service(binary, migrated_root) as (process, environment, migrated):
            assert migrated["config"]["version"] == 9
            assert migrated["config"]["settings"]["dpi"] == 1800
            assert migrated["config"]["settings"]["start_at_login"] is False
            assert migrated["config"]["profiles"]["default"]["mappings"]["middle"] == "copy"
            assert migrated["config"]["profiles"]["default"]["mappings"]["mode_shift"] == "switch_scroll_mode"
            assert (migrated_root / "config.json").read_bytes() == original
            stop(binary, migrated_root, process, environment)
        invalid_root = pathlib.Path(temporary) / "invalid"
        invalid_root.mkdir(mode=0o700)
        invalid_file = invalid_root / "config.json"
        invalid_file.write_bytes(b"{invalid existing configuration")
        invalid_environment = dict(os.environ, SUPERLIGHT_CONFIG_DIR=str(invalid_root))
        run(binary, invalid_environment, "--headless", "--background", ok=False)
        assert invalid_file.read_bytes() == b"{invalid existing configuration"
        print("Headless checks passed: startup, singleton, pause/resume, validated save, bounded input, unknown-field preservation, restart, migration, invalid-startup protection and shutdown.")


if __name__ == "__main__":
    main()
