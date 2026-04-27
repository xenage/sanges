# Python API

## Copy-paste start

```python
from tempfile import TemporaryDirectory

from sagens import Daemon

with TemporaryDirectory() as state_dir:
    with Daemon.start(state_dir=state_dir) as daemon:
        box = daemon.create_box()

        # Defaults are intentionally small: 128 MiB RAM and 128 MiB workspace.
        # Settings are stop-only, so set them before start or after stop.
        box.set("memory_mb", 256)
        box.set("fs_size_mib", 128)

        box.start()
        result = box.exec_python(["-c", "print('hello from python')"])
        print(result.stdout.decode().strip())
```

## Main classes

`Daemon` owns or connects to a host daemon.

- `Daemon.start(...)` starts a managed daemon process and returns a connected `Daemon`.
- `Daemon.connect(endpoint, admin_uuid, admin_token)` connects to an existing daemon.
- `Daemon.from_config(path)` reads a saved user config and connects.
- `daemon.create_box()` creates a durable BOX and returns `Box`.
- `daemon.list_boxes()` returns `list[BoxRecord]`.
- `daemon.get_box(box_id)` returns `Box`.
- `daemon.close()` closes the client and managed process handle.
- `daemon.quit()` shuts down the daemon.

`Box` is the high-level handle for one BOX.

- `box.record` is the latest `BoxRecord` returned by the daemon.
- `box.box_id` is the BOX UUID.
- `box.refresh()` reloads `box.record`.
- `box.set(setting, value)` updates a stopped BOX setting.
- `box.start()` boots the runtime.
- `box.stop()` stops the runtime while preserving workspace data.
- `box.remove()` removes the BOX and workspace.
- `box.exec_bash(command)` runs a bash command and captures output.
- `box.exec_python(args)` runs Python with an argv-style list.
- `box.open_bash()` and `box.open_python()` open interactive shells.
- `box.fs` exposes filesystem helpers.
- `box.checkpoint` exposes checkpoint helpers.

`BoxApiClient` is the lower-level API used by `Daemon` and `Box`. Use it directly when you already have credentials or need to build your own wrapper.

## Settings

Settings are persisted on the BOX record and must be changed while the BOX is stopped.

```python
box = daemon.create_box()

box.set("memory_mb", 256)        # Guest RAM in MiB.
box.set("fs_size_mib", 128)      # Persistent workspace disk in MiB.
box.set("cpu_cores", 1)
box.set("max_processes", 256)
box.set("network_enabled", False)

box.start()
```

`memory_mb` is RAM. `fs_size_mib` is the writable workspace disk. The read-only rootfs image that contains Python is a separate runtime artifact. The default packaged runtime is expected to boot and run Python commands with 128 MiB RAM and a 128 MiB workspace on supported hosts.

## Records and types

Most return values are frozen dataclasses from `sagens._models`.

- `BoxRecord`: `box_id`, `name`, `status`, `settings`, `runtime_usage`, `workspace_path`, timestamps, and `last_error`.
- `BoxSettings`: `cpu_cores`, `memory_mb`, `fs_size_mib`, `max_processes`, `network_enabled`.
- `BoxNumericSetting`: `current`, `max`.
- `BoxBooleanSetting`: `current`, `max`.
- `BoxStatus`: `CREATED`, `RUNNING`, `STOPPED`, `FAILED`, `REMOVING`.
- `CompletedExecution`: `exit_status`, `exit_code`, `output`, `stdout`, `stderr`.
- `ExecExit`: `kind`, optional `code`, and `success`.
- `FileNode`: `path`, `kind`, `size`, optional `digest`, optional `target`.
- `ReadFileResult`: `path`, `data`, `truncated`.
- `WorkspaceCheckpointRecord`: checkpoint summary, source checkpoint, and workspace changes.

## Filesystem

```python
box.start()

box.fs.write("/workspace/message.txt", b"hello\n")
print(box.fs.read("/workspace/message.txt").data.decode())

box.fs.mkdir("/workspace/data")
box.fs.upload("./local-dir", "/workspace/data")
box.fs.download("/workspace/message.txt", "./message.txt")

for entry in box.fs.list("/workspace"):
    print(entry.kind.value, entry.path, entry.size)
```

## Exec

```python
bash = box.exec_bash("pwd && cat /workspace/message.txt")
assert bash.exit_status.success
print(bash.stdout.decode())

python = box.exec_python(["-c", "import sys; print(sys.version_info.major)"])
assert python.exit_status.success
```

Use `exec_bash_with_timeout(command, timeout_ms, kill_grace_ms)` when the command needs a hard deadline.

## Interactive shells

```python
shell = box.open_bash()
shell.send_input("printf 'interactive-ok\\n'\nexit\n")

for event in shell.iter_events():
    if event.__class__.__name__ == "ShellOutputEvent":
        print(event.data.decode(errors="ignore"), end="")
    if event.__class__.__name__ == "ShellExitEvent":
        break
```

## Checkpoints

```python
box.fs.write("/workspace/version.txt", b"one\n")
checkpoint = box.checkpoint.create("before-change", {"source": "example"})

box.fs.write("/workspace/version.txt", b"two\n")
box.checkpoint.restore(checkpoint.summary.checkpoint_id)

forked_record = box.checkpoint.fork(checkpoint.summary.checkpoint_id, "forked-box")
```

## Box-scoped credentials

```python
bundle = daemon.issue_box_credentials(box.box_id)
box_client = daemon.connect_as_box(bundle.box_id, bundle.box_token)
```

Box-scoped clients can operate on their own BOX without receiving admin credentials.

## Errors

Daemon and transport failures raise `SagensError`.

```python
from sagens import SagensError

try:
    box.start()
except SagensError as error:
    print(error)
    box.refresh()
    print(box.record.last_error)
```
