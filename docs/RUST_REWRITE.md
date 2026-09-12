# Rust rewrite evidence

The compatibility target is upstream Mouser v3.6.0 at `34d93f70b2a84e425698e0d3d748cae2c5d18911`. The fork started at `e780641d3e709f914d6273985da9ac2ab85a7322`.

## Test-first baseline

The original repository already contains tests. Before implementing Rust, all 261 upstream v3.6.0 tests passed on macOS arm64 in run [34684699431](https://github.com/vincenthopf/SuperLight/actions/runs/34684699431). The fork's existing Ubuntu compilation, Python tests and QML lint also passed in run [34684699440](https://github.com/vincenthopf/SuperLight/actions/runs/34684699440).

`compatibility/test_v36.py` was committed before the Rust implementation. Its characterization run passed against the immutable upstream implementation in [34685131319](https://github.com/vincenthopf/SuperLight/actions/runs/34685131319). Rust CI evaluates the same requests with both implementations and compares outputs. This is separate from the original Python suite, which cannot directly exercise a Rust replacement.

## Compatibility boundaries

The protocol retains 20-byte BLE output reports, optional input report IDs, software ID 0x0A, the response-function increment quirk, direct Bluetooth and six receiver slots, model-specific DPI limits, gesture CID preference and capability selection, basic/enhanced SmartShift function IDs, and the fixed-ratchet 0xFF threshold. Configuration migrations preserve the original v1 through v9 semantics and unknown fields.

Intentional safety differences include rejecting oversized writes instead of truncating them, correlating replies and errors to the actual receiver slot and request, rejecting malformed configuration without overwriting it, bounded queues, and avoiding changes to non-Logitech devices.

## Risks and release gates

Hardware behavior is not proven by mock tests. Release requires Bluetooth and Bolt device checks, sleep/wake and receiver reconnect checks, no stuck remapped buttons, native permission and input-hook checks, and measured idle/active resource use on macOS and Windows. A macOS CI runner does not have a Logitech mouse attached. No claim that the reported 5 GB usage has been reproduced or fixed on the user's machine is made.

The old implementation remains available while native parity is being built. This branch is not a production replacement until native integration and release gates are recorded.
