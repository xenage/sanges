import assert from "node:assert/strict";

import { withDaemon, requireFullE2E } from "./helpers.mjs";

requireFullE2E();

await withDaemon(async (daemon) => {
  const box = await daemon.createBox();
  await box.start();

  await box.fs.write("/workspace/message.txt", "seed");
  const bash = await box.execBash("cat /workspace/message.txt && uname -s");
  assert.equal(bash.exitStatus.success, true);
  assert.match(bash.stdoutText, /seed/);

  const python = await box.execPython([
    "-c",
    "from pathlib import Path; Path('python.txt').write_text('python-ok'); print(Path('message.txt').read_text())"
  ]);
  assert.equal(python.exitStatus.success, true);
  assert.match(python.stdoutText, /seed/);

  const shell = await box.openBash();
  await shell.sendInput("printf 'shell-ok\\n'\nexit\n");
  let shellOutput = "";
  for await (const event of shell.iterEvents()) {
    if ("text" in event) {
      shellOutput += event.text;
    } else {
      assert.equal(event.code, 0);
    }
  }
  assert.match(shellOutput, /shell-ok/);

  const checkpoint = await box.checkpoint.create("seed", { scope: "node-e2e" });
  await box.fs.write("/workspace/message.txt", "mutated");
  await box.checkpoint.restore(checkpoint.summary.checkpointId);
  assert.equal((await box.fs.read("/workspace/message.txt")).data.toString(), "seed");

  await box.fs.write("/workspace/message.txt", "source-after-restore");
  const forkRecord = await box.checkpoint.fork(checkpoint.summary.checkpointId, "node-fork");
  const forked = await daemon.getBox(forkRecord.boxId);
  await forked.start();
  assert.equal((await forked.fs.read("/workspace/message.txt")).data.toString(), "seed");

  const bundle = await daemon.issueBoxCredentials(box.boxId);
  const boxClient = await daemon.connectAsBox(box.boxId, bundle.boxToken);
  try {
    await boxClient.writeFile(box.boxId, "/workspace/box-auth.txt", "box-only");
    assert.equal((await boxClient.readFile(box.boxId, "/workspace/box-auth.txt")).data.toString(), "box-only");
    await assert.rejects(() => boxClient.readFile(forked.boxId, "/workspace/message.txt"));
  } finally {
    boxClient.close();
  }

  await forked.stop();
  await forked.remove();
  await box.stop();
  await box.remove();
});

console.log("node full e2e passed");
