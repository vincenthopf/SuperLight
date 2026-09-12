import argparse
import json
import os
import pathlib
import subprocess
import tempfile
import time


def run(binary, environment, *arguments, ok=True):
    result = subprocess.run([str(binary), *arguments], env=environment, text=True, capture_output=True, timeout=10)
    if ok and result.returncode != 0:
        raise AssertionError(f"{arguments}: {result.returncode}\n{result.stdout}\n{result.stderr}")
    if not ok and result.returncode == 0:
        raise AssertionError(f"{arguments}: unexpectedly succeeded")
    return result


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True, type=pathlib.Path)
    arguments = parser.parse_args()
    binary = arguments.binary.resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix="superlight-verification-") as temporary:
        root = pathlib.Path(temporary)
        environment = dict(os.environ, SUPERLIGHT_CONFIG_DIR=str(root))
        output = root / "service-output.txt"
        error = root / "service-error.txt"
        with output.open("w") as stdout, error.open("w") as stderr:
            process = subprocess.Popen([str(binary), "--headless", "--background"], env=environment, stdout=stdout, stderr=stderr)
            try:
                deadline = time.monotonic() + 15
                while time.monotonic() < deadline:
                    if process.poll() is not None:
                        raise AssertionError(f"Service exited with {process.returncode}: {error.read_text()}")
                    state = run(binary, environment, "--status", ok=False) if False else subprocess.run([str(binary), "--status"], env=environment, text=True, capture_output=True, timeout=5)
                    if state.returncode == 0:
                        break
                    time.sleep(0.1)
                else:
                    raise AssertionError("Service did not publish its local endpoint before the startup deadline")
                initial = json.loads(state.stdout)["snapshot"]
                assert initial["device"] is None
                assert not initial["native_ready"]
                assert initial["config"]["version"] == 9
                first_revision = initial["revision"]
                run(binary, environment, "--headless", "--background")
                assert process.poll() is None
                run(binary, environment, "--pause")
                assert json.loads(run(binary, environment, "--status").stdout)["snapshot"]["paused"]
                run(binary, environment, "--resume")
                assert not json.loads(run(binary, environment, "--status").stdout)["snapshot"]["paused"]
                value = initial["config"]
                value["settings"]["dpi"] = 1600
                value["future_extension"] = {"must_survive": [1, 2, 3]}
                value["profiles"]["browser"] = {"label": "Browser", "apps": ["test.browser"], "mappings": {"middle": "copy"}}
                source = root / "apply.json"
                source.write_text(json.dumps(value), encoding="utf-8")
                result = json.loads(run(binary, environment, "--apply", str(source)).stdout)
                assert result["ok"]
                applied = result["snapshot"]
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
                run(binary, environment, "--quit")
                assert process.wait(timeout=10) == 0
                assert not (root / "run" / "endpoint.json").exists()
                persisted = json.loads(saved)
                assert persisted["settings"]["dpi"] == 1600
                assert persisted["profiles"]["browser"]["mappings"]["middle"] == "copy"
                print("Headless service checks passed: startup, singleton, pause, resume, validated save, unknown-field preservation, invalid-config protection and shutdown.")
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


if __name__ == "__main__":
    main()
