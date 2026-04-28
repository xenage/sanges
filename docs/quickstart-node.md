# Node quickstart

## Why this matters

Use the Node SDK when your agent product is already a JavaScript or TypeScript service. Your backend can create a BOX for a user, start it, write user input into `/workspace`, checkpoint before risky work, and then hand an agent a credential scoped to only that BOX.

The public package is small. The host binary arrives through a platform optional package, so `npm install @xenage/sanges` installs only the binary for the current host.

## Copy-paste example

```bash
npm install @xenage/sanges

node --input-type=module <<'JS'
import { Daemon } from "@xenage/sanges";

const daemon = await Daemon.start();
try {
  const box = await daemon.createBox();
  await box.start();

  await box.fs.write("/workspace/request.txt", "Summarize the uploaded CSV\n");
  const checkpoint = await box.checkpoint.create("before-agent-run");
  console.log(`checkpoint: ${checkpoint.summary.checkpointId}`);

  const bundle = await daemon.issueBoxCredentials(box.boxId);
  const agent = await daemon.connectAsBox(box.boxId, bundle.boxToken);
  try {
    await agent.execBash(
      box.boxId,
      "printf 'Agent answer for: ' > /workspace/answer.txt && cat /workspace/request.txt >> /workspace/answer.txt",
    );
    const answer = await agent.readFile(box.boxId, "/workspace/answer.txt");
    console.log(answer.data.toString().trim());
  } finally {
    agent.close();
  }

  await box.stop();
} finally {
  await daemon.close();
}
JS
```

## What just happened

`Daemon.start()` launched the bundled `sagens` host binary and connected over the same WebSocket control plane used by the CLI and Python SDK.

`createBox()` created a durable workspace for one user. `box.start()` launched the disposable microVM runtime. `issueBoxCredentials()` produced a BOX-scoped credential, so the agent client can operate inside that BOX without daemon-wide access.

## What to read next

- Understand the nouns: [Mental model](mental-model.md)
- Keep work across restarts: [Persistent workspaces](recipes/persistent-workspaces.md)
- Branch before risky work: [Checkpoints and forks](recipes/checkpoints-and-forks.md)
- Debug startup: [Troubleshooting](troubleshooting.md)
