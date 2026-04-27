# Checkpoints And Forks

## Why this matters

Agents make risky edits. Sometimes that is the point.

You want a fast way to say, "save this state, try something bold, and let me roll back or branch if it goes sideways." That is what checkpoints and forks are for.

## Copy-paste example

### CLI

```bash
BIN=<path-to-sagens>

"$BIN" start
"$BIN" box new

# Copy the BOX ID from the table above.
BOX_ID=<box-id>

"$BIN" box start "$BOX_ID"
"$BIN" box exec "$BOX_ID" bash "printf 'hello checkpoint\n' > /workspace/notes.txt"
"$BIN" box checkpoint create "$BOX_ID" --name seed --meta purpose=demo

# Copy the checkpoint ID printed above.
SEED=<checkpoint-id>

"$BIN" box exec "$BOX_ID" bash "printf 'second version\n' > /workspace/notes.txt"
"$BIN" box checkpoint restore "$BOX_ID" "$SEED" --mode rollback
"$BIN" box exec "$BOX_ID" bash "cat /workspace/notes.txt"
"$BIN" box checkpoint fork "$BOX_ID" "$SEED" --name forked-box
```

### Python

```bash
python3 - <<'PY'
from tempfile import TemporaryDirectory

from sagens import CheckpointRestoreMode, Daemon

with TemporaryDirectory() as state_dir:
    with Daemon.start(state_dir=state_dir) as daemon:
        box = daemon.create_box()
        box.start()

        box.fs.write("/workspace/notes.txt", b"hello checkpoint")
        seed = box.checkpoint.create("seed", {"purpose": "demo"})

        box.fs.write("/workspace/notes.txt", b"second version")
        box.checkpoint.restore(seed.summary.checkpoint_id, CheckpointRestoreMode.ROLLBACK)
        print(box.fs.read("/workspace/notes.txt").data.decode())

        forked = box.checkpoint.fork(seed.summary.checkpoint_id, "forked-box")
        print(forked.box_id)
PY
```

## What just happened

- `checkpoint create` captured the current workspace state into the BOX lineage.
- `checkpoint restore ... --mode rollback` moved the workspace back to the saved state.
- `checkpoint fork` created a new BOX from the checkpoint instead of mutating the source BOX in place.

The useful mental split is:

- restore when you want to rewind this BOX
- fork when you want a new BOX that starts from an older state

That makes checkpoints a practical safety rail for refactors, package upgrades, prompt experiments, and parallel agent branches.

## What to read next

- Understand why the workspace survives runtime changes: [Persistent workspaces](persistent-workspaces.md)
- Learn the public nouns once: [Mental model](../mental-model.md)
- Debug checkpoint or runtime issues: [Troubleshooting](../troubleshooting.md)
