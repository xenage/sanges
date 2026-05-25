import assert from "node:assert/strict";

import { importSagens, withDaemon } from "./helpers.mjs";

const { SagensError, resolveHostBinary } = await importSagens();

assert.ok(resolveHostBinary().endsWith("sagens"));

await withDaemon(async (daemon) => {
  const baseImage = await daemon.inspectImage("base");
  assert.equal(baseImage.name, "base");
  assert.ok(baseImage.apk.includes("python3"));

  const box = await daemon.createBox();
  assert.equal(box.record.status, "created");
  assert.equal(box.record.image, "base");
  assert.equal(box.record.settings.memoryMb.current, 128);
  assert.equal(box.record.settings.fsSizeMib.current, 128);

  const baseBox = await daemon.createBox({ image: "base" });
  assert.equal(baseBox.record.image, "base");

  const records = await daemon.listBoxes();
  assert.ok(records.some((record) => record.boxId === box.boxId));

  const bundle = await daemon.issueBoxCredentials(box.boxId);
  const boxClient = await daemon.connectAsBox(box.boxId, bundle.boxToken);
  try {
    await assert.rejects(() => boxClient.listBoxes(), SagensError);
    await assert.rejects(() => boxClient.startBox(box.boxId), SagensError);
  } finally {
    boxClient.close();
  }
});

console.log("node smoke passed");
