# CLI Quickstart

## Why this matters

This is the shortest path from "I cloned the repo" to "I have an isolated BOX with a durable workspace and I can run real commands inside it."

## Copy-paste example

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

If you are on linux/x86_64 and the first runtime start needs more RAM, set it before `box start`:

```bash
"$BIN" box set "$BOX_ID" memory_mb 3584
```

## What just happened

- `start` bootstrapped or reused the local daemon.
- `box new` created a BOX record with its own durable workspace identity.
- `box start` launched the BOX runtime.
- `box exec` ran a real guest-side shell command and wrote a file into `/workspace`.
- `box fs ... ls` showed that the file lives in the BOX workspace, not in a random host temp directory.
- `box stop` shut down the runtime without deleting the workspace.
- `quit` shut down the daemon.

If you want exact syntax for a subcommand, the fast path is built-in help:

```bash
"$BIN" help box exec
"$BIN" help box fs
```

## What to read next

- Keep state across stop and restart: [Persistent workspaces](recipes/persistent-workspaces.md)
- Add safe rollback points: [Checkpoints and forks](recipes/checkpoints-and-forks.md)
- Understand the public nouns: [Mental model](mental-model.md)
