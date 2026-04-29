# sagens Python

Python package for managing `sagens` daemons and BOX workspaces.

## What it includes

- Rust bindings for daemon lifecycle helpers and smoke-test server bootstrap
- A synchronous Python client for the `box_api` websocket protocol
- High-level classes for `Daemon`, `Box`, `BoxFs`, `BoxCheckpoint`, and `BoxShell`
- `pytest` smoke and gated full e2e coverage

## Support

- Host OS and CPU: macOS `arm64`, Linux `x86_64`, Linux `arm64` / `aarch64`
- Python versions: `3.11+` (`pyo3` `abi3-py311`; classifiers for `3.11`, `3.12`, and `3.13`)
- Linux full microVM runtime requires `/dev/kvm`
- The current libkrun-only backend does not support macOS `x86_64` or Windows hosts

## Quickstart

Build the host binary, install the package, and run Python inside a BOX:

```bash
cargo run --bin xtask -- dev --python-package-root python
python3 -m pip install -e python[test]

python3 - <<'PY'
from tempfile import TemporaryDirectory

from sagens import Daemon

with TemporaryDirectory() as state_dir:
    with Daemon.start(state_dir=state_dir) as daemon:
        box = daemon.create_box()

        # Settings are stop-only, so set them before start or after stop.
        box.set("memory_mb", 512)
        box.set("fs_size_mib", 1024)
        box.set("cpu_cores", 2)
        box.set("network_enabled", False)

        box.start()
        box.fs.write("/workspace/input.txt", b"hello from python\n")

        result = box.exec_python(
            [
                "-c",
                (
                    "from pathlib import Path; "
                    "text = Path('/workspace/input.txt').read_text(); "
                    "print(text.strip().upper())"
                ),
            ]
        )
        print(result.stdout.decode().strip())

        box.stop()
PY
```

## BOX settings examples

All BOX settings are updated with `box.set(...)` while the BOX is stopped:

```python
box.set("memory_mb", 512)       # Guest RAM in MiB.
box.set("fs_size_mib", 1024)    # Persistent workspace disk in MiB.
box.set("cpu_cores", 2)

box.set("network_enabled", True)   # Enable network.
box.set("network_enabled", False)  # Disable network.
```

## Local development

Build the host binary and stage it into the package layout:

```bash
cargo run --bin xtask -- dev --python-package-root python
```

Install the package in editable mode:

```bash
python3 -m pip install -e python[test]
```

Run smoke tests:

```bash
python3 -m pytest python/tests -q
```

Run full e2e when runtime assets and env are ready:

```bash
SAGENS_RUN_E2E=1 python3 -m pytest python/tests/test_e2e.py -q
```
