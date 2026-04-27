# sagens

One agent, one microVM, one durable workspace.

## Why this exists

Most agent runtimes make a bad tradeoff somewhere.

If every agent shares the same machine, one careless install, runaway process, or filesystem mutation leaks into everyone else's work. If you wrap everything in generic containers, you still inherit the host kernel and often end up rebuilding short-lived environments instead of preserving useful state.

`sagens` takes a different bet: give each agent its own BOX, run that BOX inside a microVM, keep the runtime disposable, and keep the workspace durable.

That means you can let agents execute untrusted code, install packages, write files, checkpoint before risky changes, and come back later without treating the whole host as scratch space.

## Two ways in

### CLI

The CLI is the fastest way to feel the product.

```bash
./build-local.sh --version local-nosign

BIN="$(find ./dist -maxdepth 1 -type f -name 'sagens-local-*' | head -n 1)"
"$BIN" start
"$BIN" box new

# Copy the BOX ID from the table above.
BOX_ID=<box-id>

"$BIN" box start "$BOX_ID"
"$BIN" box exec "$BOX_ID" bash "printf 'hello from the box\n' > /workspace/hello.txt && uname -s"
"$BIN" box fs "$BOX_ID" ls /workspace
"$BIN" box stop "$BOX_ID"
"$BIN" quit
```

### Python

The Python API is the fastest way to embed `sagens` inside an agent controller.

```bash
python3 -m pip install .

python3 - <<'PY'
from tempfile import TemporaryDirectory

from sagens import Daemon

with TemporaryDirectory() as state_dir:
    with Daemon.start(state_dir=state_dir) as daemon:
        box = daemon.create_box()
        box.start()
        box.fs.write("/workspace/hello.txt", b"hello from python\n")
        result = box.exec_bash("cat /workspace/hello.txt && uname -s")
        print(result.stdout.decode().strip())
PY
```

## Choose your path

- I want to understand the model: [Why sagens](docs/why-sagens.md)
- I want to try it from the CLI: [CLI quickstart](docs/quickstart-cli.md)
- I want to drive it from Python: [Python quickstart](docs/quickstart-python.md)
- I want durable workspaces: [Persistent workspaces](docs/recipes/persistent-workspaces.md)
- I want safe branching before risky work: [Checkpoints and forks](docs/recipes/checkpoints-and-forks.md)
- I want to scope one agent to one BOX: [Box-scoped access](docs/recipes/box-scoped-access.md)
- I need to debug startup or runtime failures: [Troubleshooting](docs/troubleshooting.md)
- I want the short version of every public noun: [Mental model](docs/mental-model.md)

## What you get

- A single host binary that acts as daemon, microVM runner, and CLI surface.
- BOXes with durable `/workspace` state and disposable runtimes.
- Guest-agent-driven exec, shell, filesystem, and checkpoint flows.
- Python bindings for the same control plane and BOX lifecycle.
- A libkrun-first, libkrun-only runtime story instead of a pile of compatibility paths.

## Need detailed command help?

Use the built-in help pages:

```bash
sagens help
sagens help box exec
sagens help box checkpoint
```

The product docs stay focused on outcomes and examples. The CLI help stays focused on exact command syntax.
