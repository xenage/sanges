# Why sagens

## Why this matters

The hard part of agent infrastructure is not "how do I run a command."

The hard part is giving agents enough freedom to do useful work without turning your host into shared, long-lived collateral damage. Agents want to install packages, edit files, spawn subprocesses, and retry dangerous operations. You still want isolation, repeatability, and a way to keep useful state.

`sagens` is built around that exact problem. Each agent gets its own BOX. Each BOX runs inside a microVM. The runtime can come and go. The workspace stays.

## Copy-paste example

```bash
./build-local.sh --version local-nosign

BIN="$(find ./dist -maxdepth 1 -type f -name 'sagens-local-*' | head -n 1)"
"$BIN" start
"$BIN" box new

# Copy the BOX ID from the table above.
BOX_ID=<box-id>

"$BIN" box start "$BOX_ID"
"$BIN" box exec "$BOX_ID" bash "printf 'draft\n' > /workspace/notes.txt"
"$BIN" box checkpoint create "$BOX_ID" --name before-risky-change
"$BIN" box stop "$BOX_ID"
"$BIN" quit
```

## What just happened

You did not create a disposable dev container. You created a durable unit of work.

- The daemon is the local control plane.
- The BOX is the durable identity an agent owns.
- The runtime is the microVM that exists only while the BOX is running.
- The workspace under `/workspace` is the state that survives stop, restart, and checkpoint operations.
- The guest agent handles exec, shell, filesystem, and checkpoint requests from the host side instead of asking you to SSH into a machine.

That is the central tradeoff:

- Shared machine model: fast to start, easy to leak state.
- Generic container model: better packaging, still a shared host-kernel story.
- `sagens` model: a heavier runtime boundary, but a much cleaner place to let agents do real work.

At the implementation level, `sagens` is intentionally opinionated: one host binary as the product surface, guest-agent-driven execution, and a libkrun-only backend instead of a matrix of compatibility layers.

## What to read next

- Try the full lifecycle from the CLI: [CLI quickstart](quickstart-cli.md)
- Drive the same model from code: [Python quickstart](quickstart-python.md)
- Learn the nouns once: [Mental model](mental-model.md)
