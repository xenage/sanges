import { resolve } from "node:path";

import { Box, Daemon, type VmImageManifest } from "../src/index.js";

const IMAGE_NAME = "chromium";
const STATE_DIR = resolve(process.env.SAGENS_EXAMPLE_STATE_DIR ?? ".sagens-node-named-images");
const CHROMIUM_SMOKE =
  "chromium --headless --no-sandbox --disable-gpu --dump-dom 'data:text/html,<h1>ok</h1>'";

async function smokeChromium(box: Box): Promise<void> {
  await box.set("memory_mb", 512);
  await box.set("max_processes", 512);
  await box.start();
  const result = await box.execBash(CHROMIUM_SMOKE);
  await box.stop();
  if (!result.stdoutText.includes("<h1>ok</h1>")) {
    throw new Error(`chromium smoke failed in ${box.boxId}: ${result.stderrText}`);
  }
}

async function ensureChromiumImage(daemon: Daemon): Promise<VmImageManifest> {
  const images = await daemon.listImages();
  const existing = images.find((image) => image.name === IMAGE_NAME);
  if (existing) {
    return existing;
  }
  return daemon.buildImage({
    name: IMAGE_NAME,
    apk: ["chromium"],
    minImageMib: 1024
  });
}

async function main(): Promise<void> {
  const daemon = await Daemon.start({ stateDir: STATE_DIR });
  const image = await ensureChromiumImage(daemon);
  const first = await daemon.createBox({ image: image.name });
  const second = await daemon.createBox({ image: image.name });

  await smokeChromium(first);
  await smokeChromium(second);
  await daemon.close();

  console.log(`created two BOXes from named image '${image.name}'`);
}

await main();
