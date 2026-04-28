# Docs

## Why this matters

If you are evaluating `sagens`, you usually do not want a repo tour. You want to know how quickly you can give a user-facing agent its own safe place to work.

This docs set is organized around that outcome: create a BOX for a user, run agent work safely, keep the workspace, checkpoint before risky changes, and hand one BOX to one agent when you need tighter scope.

## Choose your path

- I want the product thesis first: [Why sagens](why-sagens.md)
- I want the fastest CLI path: [CLI quickstart](quickstart-cli.md)
- I want the fastest Python path: [Python quickstart](quickstart-python.md)
- I want Python classes, types, and settings: [Python API](python-api.md)
- I want the public nouns in one page: [Mental model](mental-model.md)
- I want the workspace to survive restarts: [Persistent workspaces](recipes/persistent-workspaces.md)
- I want safe rollback and branching: [Checkpoints and forks](recipes/checkpoints-and-forks.md)
- I want one agent credential per user BOX: [Box-scoped access](recipes/box-scoped-access.md)
- I need to debug the control plane or runtime: [Troubleshooting](troubleshooting.md)

## Copy-paste example

### CLI

```bash
./build-local.sh --version local-nosign

BIN="$(find ./dist -maxdepth 1 -type f -name 'sagens-local-*' | head -n 1)"
"$BIN" start
"$BIN" box new
```

### Python

```bash
python3 -m pip install .

python3 - <<'PY'
from tempfile import TemporaryDirectory

from sagens import Daemon

with TemporaryDirectory() as state_dir:
    with Daemon.start(state_dir=state_dir) as daemon:
        print(daemon.create_box().box_id)
PY
```

## What just happened

Both entry points talk to the same product model.

The CLI is a direct way to operate the daemon and BOX lifecycle by hand. The Python API is the same control plane packaged for your product backend. In both cases you are creating a durable user BOX, not a throwaway shell session.

## What to read next

- Start from the command line: [CLI quickstart](quickstart-cli.md)
- Start from code: [Python quickstart](quickstart-python.md)
- Use Python as an API: [Python API](python-api.md)
- Understand the tradeoff: [Why sagens](why-sagens.md)
