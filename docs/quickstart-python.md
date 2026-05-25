# Python Quickstart

## Why this matters

If you are building an agent controller, scheduler, or developer tool, the Python API is the shortest path from idea to integration.

You get the same daemon, BOX, filesystem, exec, and checkpoint model as the CLI, but you can drive it directly from your own code.

## Copy-paste example

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

## What just happened

- `Daemon.start(...)` launched a local daemon and bootstrapped temporary state.
- `create_box()` created a durable BOX object.
- `start()` brought up the BOX runtime.
- `box.fs.write(...)` wrote into the BOX workspace.
- `exec_bash(...)` ran inside the guest and returned captured stdout and stderr.
- Exiting the context manager shut down the daemon that `Daemon.start(...)` spawned.

If you already have an existing daemon, you do not need to spawn a new one. You can connect with `Daemon.connect(...)` or load a saved config with `Daemon.from_config(...)`.

## Named Images

Use named images when every BOX should start with the same package set. The SDK builds the image through the daemon protocol; it does not shell out to the CLI.

```python
from tempfile import TemporaryDirectory

from sagens import Daemon

with TemporaryDirectory() as state_dir:
    with Daemon.start(state_dir=state_dir) as daemon:
        image = daemon.build_image(
            "chromium",
            apk=["chromium"],
            min_image_mib=1024,
        )
        first = daemon.create_box(image=image.name)
        second = daemon.create_box(image=image.name)
        print(first.record.image, second.record.image)
```

For a full Chromium smoke, see [`python/examples/named_images.py`](../python/examples/named_images.py).

## What to read next

- Keep state across restart: [Persistent workspaces](recipes/persistent-workspaces.md)
- Create rollback points and forks: [Checkpoints and forks](recipes/checkpoints-and-forks.md)
- Scope one agent to one BOX: [Box-scoped access](recipes/box-scoped-access.md)
