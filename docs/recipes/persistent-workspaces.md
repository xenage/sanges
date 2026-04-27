# Persistent Workspaces

## Why this matters

This is one of the main reasons to use `sagens` at all.

You want the runtime to be disposable, but you do not want useful agent work to disappear every time you stop, restart, or recover a BOX.

## Copy-paste example

### CLI

```bash
BIN=<path-to-sagens>

"$BIN" start
"$BIN" box new

# Copy the BOX ID from the table above.
BOX_ID=<box-id>

"$BIN" box start "$BOX_ID"
"$BIN" box exec "$BOX_ID" bash "printf 'persisted\n' > /workspace/message.txt"
"$BIN" box stop "$BOX_ID"
"$BIN" box start "$BOX_ID"
"$BIN" box exec "$BOX_ID" bash "cat /workspace/message.txt"
```

### Python

```bash
python3 - <<'PY'
from tempfile import TemporaryDirectory

from sagens import Daemon

with TemporaryDirectory() as state_dir:
    with Daemon.start(state_dir=state_dir) as daemon:
        box = daemon.create_box()
        box.start()
        box.fs.write("/workspace/message.txt", b"persisted\n")
        box.stop()
        box.start()
        print(box.fs.read("/workspace/message.txt").data.decode().strip())
PY
```

## What just happened

The runtime came and went. The workspace did not.

That is the intended contract:

- stopping a BOX should stop compute, not erase work
- starting again should reattach the same workspace identity
- if a runtime disappears unexpectedly, the BOX can move to `failed`, but an explicit `box start` should bring compute back without dropping workspace state

This is the difference between "an isolated shell session" and "a durable agent workspace."

## What to read next

- Add rollback points before risky work: [Checkpoints and forks](checkpoints-and-forks.md)
- Learn the nouns behind the behavior: [Mental model](../mental-model.md)
- Debug restart or failure cases: [Troubleshooting](../troubleshooting.md)
