import assert from "node:assert/strict";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

export async function importSagens() {
  return import(process.env.SAGENS_NODE_IMPORT ?? "@xenage/sanges");
}

export async function withDaemon(callback) {
  const { Daemon } = await importSagens();
  const stateDir = await mkdtemp(join(tmpdir(), "sagens-node-test-"));
  const daemon = await Daemon.start({ stateDir });
  try {
    return await callback(daemon, stateDir);
  } finally {
    await daemon.close().catch(() => {});
    await rm(stateDir, { recursive: true, force: true });
  }
}

export function realRuntimeSupported() {
  if (truthy(process.env.SAGENS_FORCE_REAL_BOX_E2E)) {
    return true;
  }
  if (process.platform === "darwin") {
    return process.arch === "arm64";
  }
  if (process.platform === "linux") {
    return true;
  }
  return false;
}

export function requireFullE2E() {
  assert.ok(truthy(process.env.SAGENS_RUN_E2E), "set SAGENS_RUN_E2E=1");
  assert.ok(realRuntimeSupported(), "full e2e requires linux or macos/arm64");
}

function truthy(value) {
  return ["1", "true", "yes", "on"].includes(String(value ?? "").toLowerCase());
}
