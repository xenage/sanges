import { accessSync, constants } from "node:fs";
import { createRequire } from "node:module";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { SagensError } from "./errors.js";

const require = createRequire(import.meta.url);

export function resolveHostBinary(explicitPath?: string): string {
  const configured =
    explicitPath || process.env.SAGENS_NODE_HOST_BINARY || process.env.SAGENS_HOST_BINARY;
  if (configured) {
    return assertExecutable(resolve(configured), "configured sagens host binary");
  }

  const optionalPackage = optionalBinaryPath();
  if (optionalPackage) {
    return optionalPackage;
  }

  const repoBinary = repoBuildBinary();
  if (repoBinary) {
    return repoBinary;
  }

  throw new SagensError(
    `No sagens host binary for ${process.platform}/${process.arch}. ` +
      `Install the matching @xenage/sanges-* optional package or set SAGENS_NODE_HOST_BINARY.`
  );
}

export function platformPackageName(): string {
  if (process.platform === "darwin" && process.arch === "arm64") {
    return "@xenage/sanges-darwin-arm64";
  }
  if (process.platform === "linux" && process.arch === "x64") {
    return "@xenage/sanges-linux-x64";
  }
  if (process.platform === "linux" && process.arch === "arm64") {
    return "@xenage/sanges-linux-arm64";
  }
  throw new SagensError(`Unsupported sagens platform: ${process.platform}/${process.arch}`);
}

function optionalBinaryPath(): string | undefined {
  try {
    return assertExecutable(
      require.resolve(`${platformPackageName()}/bin/sagens`),
      "platform sagens host binary"
    );
  } catch (error) {
    if (error instanceof SagensError) {
      throw error;
    }
    return undefined;
  }
}

function repoBuildBinary(): string | undefined {
  const here = dirname(fileURLToPath(import.meta.url));
  for (const candidate of [
    resolve(here, "../../target/release/sagens"),
    resolve(here, "../../target/debug/sagens"),
    resolve(here, "../../../target/release/sagens"),
    resolve(here, "../../../target/debug/sagens")
  ]) {
    try {
      return assertExecutable(candidate, "repo sagens host binary");
    } catch {
      continue;
    }
  }
  return undefined;
}

function assertExecutable(path: string, label: string): string {
  try {
    accessSync(path, constants.X_OK);
  } catch (error) {
    throw new SagensError(`${label} is not executable: ${path}`);
  }
  return path;
}
