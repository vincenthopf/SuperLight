import argparse
import copy
import json
import random
import subprocess
import sys
from contextlib import redirect_stdout
from io import StringIO
from pathlib import Path
from types import SimpleNamespace


BASELINE = "34d93f70b2a84e425698e0d3d748cae2c5d18911"


def cases(root):
    sys.path.insert(0, str(root.resolve()))
    with redirect_stdout(StringIO()):
        from core import config, hid_gesture as hid, logi_devices as devices

    rng = random.Random(360)
    rows = []

    def add(request, expected):
        rows.append((request, expected))

    for device in [255, 1, 2, 3, 4, 5, 6]:
        for function in range(16):
            for size in [0, 1, 3, 4, 16]:
                feature = rng.randrange(1, 254)
                params = [rng.randrange(256) for _ in range(size)]
                listener = hid.HidGestureListener()
                sent = []
                listener._dev_idx = device
                listener._dev = SimpleNamespace(write=lambda packet: sent.append(packet))
                listener._tx(hid.SHORT_ID, feature, function, params)
                assert len(sent[0]) == 20 and sent[0][0] == 17
                add({"op": "encode", "device": device, "feature": feature, "function": function, "params": params}, sent[0])
                for raw in [sent[0], sent[0][1:]]:
                    parsed = hid._parse(raw)
                    add({"op": "parse", "raw": raw}, list(parsed))
    for raw in [[], [17], [17, 255], [255, 2, 10]]:
        assert hid._parse(raw) is None
        add({"op": "parse", "raw": raw}, None)

    for enhanced in [False, True]:
        for enabled in [False, True]:
            for mode in ["ratchet", "freespin"]:
                for threshold in [-100, 0, 1, 25, 50, 51, 255]:
                    listener = hid.HidGestureListener()
                    listener._dev = object()
                    listener._smart_shift_idx = 7
                    listener._smart_shift_enhanced = enhanced
                    listener._pending_smart_shift = (mode, enabled, threshold)
                    sent = []

                    def request(feature, function, params):
                        sent.append({"feature": feature, "function": function, "params": params})
                        return (255, feature, function, 10, [0] * 16)

                    listener._request = request
                    with redirect_stdout(StringIO()):
                        listener._apply_pending_smart_shift()
                    assert listener._smart_shift_result is True
                    add({"op": "smart_write", "enhanced": enhanced, "mode": mode, "enabled": enabled, "threshold": threshold}, sent[0])
        for mode in [0, 1, 2, 3]:
            for threshold in range(256):
                listener = hid.HidGestureListener()
                listener._dev = object()
                listener._smart_shift_idx = 7
                listener._smart_shift_enhanced = enhanced
                listener._request = lambda *_: (255, 7, 0, 10, [mode, threshold])
                with redirect_stdout(StringIO()):
                    listener._apply_pending_read_smart_shift()
                add({"op": "smart_read", "mode": mode, "threshold": threshold}, listener._smart_shift_result)

    for spec in [None, *devices.KNOWN_LOGI_DEVICES]:
        pid = spec.product_ids[0] if spec else 0
        for value in [-10000, 0, 199, 200, 800, 1000, 4000, 8000, 10000, 65535]:
            add({"op": "dpi", "pid": pid, "value": value}, devices.clamp_dpi(value, spec))
        if spec:
            for name in [spec.key, spec.display_name, *spec.aliases]:
                resolved = devices.resolve_device(product_name=name)
                assert resolved is not None
                add({"op": "device", "pid": 0, "name": name}, {"key": resolved.key, "name": resolved.display_name, "min": resolved.dpi_min, "max": resolved.dpi_max})

    control_sets = [
        [],
        [{"cid": 215, "flags": 944, "mapping_flags": 81}, {"cid": 195, "flags": 304, "mapping_flags": 17}],
        [{"cid": 160, "flags": 48, "mapping_flags": 1}, {"cid": 241, "flags": 432, "mapping_flags": 17}],
        [{"cid": 195, "flags": 0, "mapping_flags": 0}],
        [{"cid": 241, "flags": 128, "mapping_flags": 16}],
    ]
    for controls in control_sets:
        for pid in [0, 45091, 45088]:
            listener = hid.HidGestureListener()
            expected = listener._choose_gesture_candidates(controls, device_spec=devices.resolve_device(product_id=pid))
            add({"op": "candidates", "controls": controls, "pid": pid}, expected)

    for version in range(1, 10):
        for action in ["none", "toggle_smart_shift", "switch_scroll_mode", "custom:cmd+shift+3"]:
            source = {"version": version, "active_profile": "default", "profiles": {"default": {"label": "Default", "apps": ["wmplayer.exe"], "mappings": {"mode_shift": action}}, "work": {"label": "Work", "apps": [], "mappings": {"middle": "mouse_left_click", "mode_shift": action}}}, "settings": {"start_with_windows": True, "smart_shift_mode": "freespin", "smart_shift_enabled": True}}
            expected = config._validate_types(config._merge_defaults(config._migrate(copy.deepcopy(source)), copy.deepcopy(config.DEFAULT_CONFIG)), config.DEFAULT_CONFIG)
            add({"op": "migrate", "config": source}, expected)
    add({"op": "migrate", "config": {}}, config._validate_types(config._merge_defaults(config._migrate({}), copy.deepcopy(config.DEFAULT_CONFIG)), config.DEFAULT_CONFIG))
    return rows


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--baseline-dir", required=True, type=Path)
    parser.add_argument("--binary", type=Path)
    args = parser.parse_args()
    revision = subprocess.check_output(["git", "-C", str(args.baseline_dir), "rev-parse", "HEAD"], text=True).strip()
    if revision != BASELINE:
        raise SystemExit(f"Wrong baseline: {revision}")
    rows = cases(args.baseline_dir)
    if args.binary:
        payload = "".join(json.dumps(request, separators=(",", ":")) + "\n" for request, _ in rows)
        result = subprocess.run([str(args.binary.resolve())], input=payload, text=True, capture_output=True, timeout=60, check=True)
        actual = result.stdout.splitlines()
        if len(actual) != len(rows):
            raise AssertionError(f"Expected {len(rows)} responses, got {len(actual)}: {result.stderr}")
        for index, ((request, expected), line) in enumerate(zip(rows, actual)):
            decoded = json.loads(line)
            if decoded != expected:
                raise AssertionError(f"Contract {index}: {request}\nPython: {expected}\nRust: {decoded}")
        print(f"PASS: {len(rows)} differential contracts against immutable v3.6.0")
    else:
        print(f"PASS: characterized {len(rows)} contracts from immutable v3.6.0 before Rust implementation")


if __name__ == "__main__":
    main()
