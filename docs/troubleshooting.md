# Troubleshooting

## Why this matters

When `sagens` feels broken, the failure is usually not mysterious. It is usually one of a few product-level issues: the daemon did not start cleanly, the BOX runtime failed to come up, the runtime disappeared and now needs an explicit restart, or a BOX setting was changed while the BOX was still running.

## Copy-paste example

```bash
BIN=<path-to-sagens>

"$BIN" start
"$BIN" box list
"$BIN" daemon log --tail 200
"$BIN" help box start
```

## What just happened

Use the checklist below.

### The daemon will not start cleanly

Run `sagens start` first. If it still fails, inspect:

```bash
sagens daemon log --tail 200
```

The daemon log is the first place to look for startup failures and control-plane issues.

### A BOX will not start

Start with the same daemon log:

```bash
sagens daemon log --tail 200
```

When BOX startup fails, the log records the related runner and guest console log paths. That is the quickest route to the real failure.

On Linux x86_64, the packaged libkrun kernel path currently needs more RAM than the tiny default. Set the BOX to at least `3584 MiB` before start if the daemon reports that the raw bundled kernel needs at least `3329 MiB`.

### A BOX suddenly shows `failed`

That usually means the runtime went away and the BOX now needs an explicit restart. Start it again:

```bash
sagens box start <BOX_ID>
```

The important contract is that the workspace should still be there even though the runtime was lost.

### A settings update is rejected

Some settings are intentionally stop-only. Stop the BOX first, then update it:

```bash
sagens box stop <BOX_ID>
sagens box set <BOX_ID> memory_mb 2048
```

That applies to settings such as CPU, RAM, filesystem size, process count, and network.

### The local build does not work on this host

The current local build path supports:

- macOS arm64
- Linux aarch64
- Linux x86_64

The libkrun-only backend does not support local macOS x86_64 host builds.

### The Python e2e path does not behave like the real runtime

The full real-runtime path expects either:

- Linux with `/dev/kvm`
- macOS arm64

On unsupported hosts, use the smoke-level Python flow and treat full microVM runtime validation as a separate environment concern.

## What to read next

- Understand the main tradeoff: [Why sagens](why-sagens.md)
- Re-run the normal lifecycle from scratch: [CLI quickstart](quickstart-cli.md)
- Use persistence and checkpoints intentionally: [Persistent workspaces](recipes/persistent-workspaces.md) and [Checkpoints and forks](recipes/checkpoints-and-forks.md)
