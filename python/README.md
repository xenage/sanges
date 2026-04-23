# sagens Python

Python package for managing `sagens` daemons and BOX workspaces.

## What it includes

- Rust bindings for daemon lifecycle helpers and smoke-test server bootstrap
- A synchronous Python client for the `box_api` websocket protocol
- High-level classes for `Daemon`, `Box`, `BoxFs`, `BoxCheckpoint`, and `BoxShell`
- `pytest` smoke and gated full e2e coverage

## Local development

Build the host binary and stage it into the package layout:

```bash
rtk cargo run --bin xtask -- dev --python-package-root python
```

Install the package in editable mode:

```bash
rtk python3 -m pip install -e python[test]
```

Run smoke tests:

```bash
rtk python3 -m pytest python/tests -q
```

Run full e2e when runtime assets and env are ready:

```bash
rtk SAGENS_RUN_E2E=1 python3 -m pytest python/tests/test_e2e.py -q
```
