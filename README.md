# sagens

Give every user-facing agent its own isolated BOX: a microVM-backed workspace that can run code, keep state, and be handed to exactly one agent.

## Why this exists

If you are building agents for yourself, a terminal and a local checkout are enough.

If you are building agentic chat for other people, the shape changes. Every user may ask an agent to install packages, write files, run scripts, inspect data, retry failed work, and come back tomorrow. You need the agent to be useful without letting one user's runtime, filesystem, or mistakes leak into another user's session.

`sagens` is the infrastructure primitive for that product: create a BOX for a user, attach an agent to that BOX, let the agent work inside a microVM, persist `/workspace`, and checkpoint before risky changes.

The bet is simple: a user session should not be a shared shell. It should be a durable BOX.

## Supported hosts and SDK runtimes

`sagens` currently publishes and tests the following host matrix:

| Host OS | CPU | Python package | Node package | microVM backend |
| --- | --- | --- | --- | --- |
| macOS | arm64 (Apple Silicon) | Python 3.11+ | Node 20+ | vendored `libkrun` on Apple's Hypervisor Framework (HVF) with bundled `KRUN_EFI.silent.fd` firmware |
| Linux | x86_64 | Python 3.11+ | Node 20+ | vendored `libkrun` on KVM with a guest kernel materialized from `libkrunfw-x86_64` |
| Linux | arm64 / aarch64 | Python 3.11+ | Node 20+ | vendored `libkrun` on KVM with a guest kernel materialized from `libkrunfw-aarch64` |

Notes:

- Linux full microVM runtime requires `/dev/kvm`.
- The Python package requires `>=3.11`, is built with `pyo3` `abi3-py311`, and is classified for Python `3.11`, `3.12`, and `3.13`.
- The Node package declares `"engines": { "node": ">=20" }`. CI currently exercises Node `22`.
- Windows and macOS `x86_64` are not supported by the current libkrun-only backend.
- For the per-host runtime breakdown, see [Support matrix](docs/support-matrix.md).

## What you can build

- A coding agent product where each customer gets an isolated Linux workspace.
- A support or data-analysis chat where uploaded files stay inside that user's BOX.
- A multi-agent system where each worker receives only the BOX credentials it needs.
- A hosted agent lab where runtimes are disposable but user work survives stop, restart, and restore.

## Three ways in

### Python: create a BOX and hand it to an agent

The Python API is the path for product code: your backend creates a user BOX, starts it, stores user input, issues BOX-scoped credentials, and connects an agent client that can only act on that BOX.

```bash
python3 -m pip install sagens

python3 - <<'PY'
from tempfile import TemporaryDirectory

from sagens import Daemon

with TemporaryDirectory() as state_dir:
    with Daemon.start(state_dir=state_dir) as daemon:
        user_box = daemon.create_box()
        user_box.start()
        user_box.fs.write(
            "/workspace/request.txt",
            b"User asked for a pricing analysis\n",
        )

        checkpoint = user_box.checkpoint.create(name="before-agent-run")
        print(f"safe restore point: {checkpoint.summary.checkpoint_id}")

        bundle = daemon.issue_box_credentials(user_box.box_id)
        agent = daemon.connect_as_box(user_box.box_id, bundle.box_token)
        try:
            # Replace this command with your agent process.
            agent.exec_bash(
                user_box.box_id,
                "printf 'Agent answer for: ' > /workspace/answer.txt && "
                "cat /workspace/request.txt >> /workspace/answer.txt",
            )
            answer = agent.read_file(
                user_box.box_id,
                "/workspace/answer.txt",
                16 * 1024 * 1024,
            )
            print(answer.data.decode().strip())
        finally:
            agent.close()
            user_box.stop()
PY
```

That is the product boundary: your control plane can manage many BOXes, while each agent can receive a credential scoped to only one user's BOX.

### Node: the same product boundary from JavaScript

The Node package ships the SDK and installs the right platform host binary through npm optional dependencies. Your users do not build libkrun, download runtime assets, or share a host shell.

```bash
npm install @xenage/sanges

node --input-type=module <<'JS'
import { Daemon } from "@xenage/sanges";

const daemon = await Daemon.start();
try {
  const userBox = await daemon.createBox();
  await userBox.start();
  await userBox.fs.write(
    "/workspace/request.txt",
    "User asked for a pricing analysis\n",
  );

  const checkpoint = await userBox.checkpoint.create("before-agent-run");
  console.log(`safe restore point: ${checkpoint.summary.checkpointId}`);

  const bundle = await daemon.issueBoxCredentials(userBox.boxId);
  const agent = await daemon.connectAsBox(userBox.boxId, bundle.boxToken);
  try {
    await agent.execBash(
      userBox.boxId,
      "printf 'Agent answer for: ' > /workspace/answer.txt && " +
        "cat /workspace/request.txt >> /workspace/answer.txt",
    );
    const answer = await agent.readFile(userBox.boxId, "/workspace/answer.txt");
    console.log(answer.data.toString().trim());
  } finally {
    agent.close();
    await userBox.stop();
  }
} finally {
  await daemon.close();
}
JS
```

The public package is `@xenage/sanges`; platform binary packages such as `@xenage/sanges-darwin-arm64` are pulled automatically by npm for supported hosts.

### CLI: create a user BOX by hand

The CLI is the fastest way to see the product model end to end. This example creates one BOX for one user, writes a request into the workspace, runs an agent-shaped command, checkpoints the result, and stops the runtime while keeping the workspace.

```bash
./build-local.sh --version local-nosign

BIN="$(find ./dist -maxdepth 1 -type f -name 'sagens-local-*' | head -n 1)"
"$BIN" start
"$BIN" box new

# Copy the BOX ID from the table above.
BOX_ID=<box-id>

"$BIN" box start "$BOX_ID"
"$BIN" box exec "$BOX_ID" bash "printf 'User asked for a pricing analysis\n' > /workspace/request.txt"
"$BIN" box exec "$BOX_ID" bash "printf 'Agent answer for: ' > /workspace/answer.txt && cat /workspace/request.txt >> /workspace/answer.txt"
"$BIN" box fs "$BOX_ID" ls /workspace
"$BIN" box checkpoint create "$BOX_ID" --name after-first-agent-answer
"$BIN" box stop "$BOX_ID"
```

Replace the second `box exec` with your real agent runner. The important part is that the agent is working inside the user's BOX, not on your host and not in a shared workspace.

You can then start the same BOX again to run Python, and reconfigure it while it is stopped:

```bash
"$BIN" box start "$BOX_ID"
"$BIN" box exec "$BOX_ID" python -c "from pathlib import Path; print(Path('/workspace/request.txt').read_text().strip())"

"$BIN" box stop "$BOX_ID"
"$BIN" box set "$BOX_ID" memory 512MiB
"$BIN" box set "$BOX_ID" fs 1GiB
"$BIN" box set "$BOX_ID" cpu 2

# Choose one of the following lines.
"$BIN" box set "$BOX_ID" network enabled
"$BIN" box set "$BOX_ID" network disabled

"$BIN" quit
```

The `memory`, `fs`, `cpu`, and `network` names above are short aliases for `memory_mb`, `fs_size_mib`, `cpu_cores`, and `network_enabled`.


## Choose your path

- I want to understand the model: [Why sagens](docs/why-sagens.md)
- I need the support matrix first: [Support matrix](docs/support-matrix.md)
- I want to try it from the CLI: [CLI quickstart](docs/quickstart-cli.md)
- I want to drive it from Python: [Python quickstart](docs/quickstart-python.md)
- I want to drive it from Node: [Node quickstart](docs/quickstart-node.md)
- I want the Python API surface: [Python API](docs/python-api.md)
- I want durable user workspaces: [Persistent workspaces](docs/recipes/persistent-workspaces.md)
- I want safe branching before risky agent work: [Checkpoints and forks](docs/recipes/checkpoints-and-forks.md)
- I want to scope one agent to one BOX: [Box-scoped access](docs/recipes/box-scoped-access.md)
- I need to debug startup or runtime failures: [Troubleshooting](docs/troubleshooting.md)
- I want the short version of every public noun: [Mental model](docs/mental-model.md)

## What you get

- One host binary for daemon, microVM runner, and CLI.
- BOXes with durable `/workspace` state and disposable runtimes.
- Guest-agent-driven exec, shell, filesystem, and checkpoint flows.
- Python and Node SDKs for creating user BOXes and attaching agents to them.
- BOX-scoped credentials so one agent does not need daemon-wide access.
- A libkrun-first, libkrun-only runtime story instead of Docker, QEMU, or shared-machine fallbacks.

## Detailed command help

The docs focus on outcomes and examples. The CLI keeps exact syntax close to the binary:

```bash
sagens help
sagens help box exec
sagens help box set
sagens help box checkpoint
```
