import assert from "node:assert/strict";

import { importSagens, withDaemon } from "./helpers.mjs";

const { SagensError, resolveHostBinary } = await importSagens();

assert.ok(resolveHostBinary().endsWith("sagens"));

await withDaemon(async (daemon) => {
  const box = await daemon.createBox();
  assert.equal(box.record.status, "created");
  assert.equal(box.record.settings.memoryMb.current, 128);
  assert.equal(box.record.settings.fsSizeMib.current, 128);

  const records = await daemon.listBoxes();
  assert.ok(records.some((record) => record.boxId === box.boxId));

  const bundle = await daemon.issueBoxCredentials(box.boxId);
  const boxClient = await daemon.connectAsBox(box.boxId, bundle.boxToken);
  try {
    await assert.rejects(() => boxClient.listBoxes(), SagensError);
  } finally {
    boxClient.close();
  }
});

console.log("node smoke passed");
