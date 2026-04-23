# sagens

Self-hosted per-agent microVMs for securely running untrusted code.

## Python

Install the standalone Python package from the repo root:

```bash
pip install .
```

Start a daemon and talk to it from Python:

```python
from tempfile import TemporaryDirectory

from sagens import Daemon

with TemporaryDirectory() as state_dir:
    with Daemon.start(state_dir=state_dir) as daemon:
        print(daemon.list_boxes())
```

Create and start 10 boxes:

```python
from tempfile import TemporaryDirectory

from sagens import Daemon

with TemporaryDirectory() as state_dir:
    with Daemon.start(state_dir=state_dir) as daemon:
        boxes = [daemon.create_box() for _ in range(10)]
        for box in boxes:
            box.start()
        print([box.refresh().status.value for box in boxes])
```

Run shell and Python commands inside boxes:

```python
from tempfile import TemporaryDirectory

from sagens import Daemon

with TemporaryDirectory() as state_dir:
    with Daemon.start(state_dir=state_dir) as daemon:
        boxes = [daemon.create_box() for _ in range(10)]
        for index, box in enumerate(boxes):
            box.start()
            box.fs.write("/workspace/message.txt", f"box-{index}\n".encode())

        for index, box in enumerate(boxes):
            shell = box.exec_bash("cat /workspace/message.txt && uname -s")
            python = box.exec_python(["-c", "print('hello from python')"])
            print(index, shell.stdout.decode().strip(), python.stdout.decode().strip())
```
