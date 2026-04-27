# Box-Scoped Access

## Why this matters

Sometimes an agent should be able to work inside one BOX and nothing else.

That is the point of box-scoped credentials: give a worker access to one durable workspace without giving it daemon-wide visibility or control.

## Copy-paste example

```bash
python3 -m pip install .

python3 - <<'PY'
from tempfile import TemporaryDirectory

from sagens import Daemon, SagensError

with TemporaryDirectory() as state_dir:
    with Daemon.start(state_dir=state_dir) as daemon:
        box = daemon.create_box()
        box.start()

        bundle = daemon.issue_box_credentials(box.box_id)
        box_client = daemon.connect_as_box(box.box_id, bundle.box_token)

        try:
            box_client.list_boxes()
        except SagensError as error:
            print(f"daemon-wide list rejected: {error}")

        result = box_client.exec_bash(box.box_id, "printf 'only this box\\n'")
        print(result.stdout.decode().strip())
        box_client.close()
PY
```

## What just happened

- `issue_box_credentials(...)` created a BOX-scoped bundle with `box_id`, `box_token`, and `endpoint`.
- `connect_as_box(...)` authenticated as that BOX instead of as a daemon admin.
- The BOX-scoped client could act on that BOX, but daemon-wide operations such as `list_boxes()` were rejected.

That is the right primitive when you want to delegate one sandbox to one worker without handing out full control-plane credentials.

Today, `sagens admin add` is the CLI path for daemon-wide admin credentials. BOX-scoped credential issuance is exposed through the BOX API and Python client surface.

## What to read next

- Start from the daemon-wide model first: [Mental model](../mental-model.md)
- Create safe rollback points inside one BOX: [Checkpoints and forks](checkpoints-and-forks.md)
- See the Python control flow end to end: [Python quickstart](../quickstart-python.md)
