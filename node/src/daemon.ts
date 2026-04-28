import { spawn, ChildProcess } from "node:child_process";
import { randomBytes, randomUUID } from "node:crypto";
import { closeSync, openSync } from "node:fs";
import { mkdir, readFile, writeFile, chmod } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import net from "node:net";

import { Box } from "./box.js";
import { BoxApiClient } from "./client.js";
import { userConfigFromWire } from "./decode.js";
import { SagensError } from "./errors.js";
import { BoxCredentialBundle, BoxRecord, UserConfig } from "./models.js";
import { resolveHostBinary } from "./binary.js";

export interface DaemonStartOptions {
  hostBinary?: string;
  stateDir?: string;
  userConfigPath?: string;
  endpoint?: string;
}

export class Daemon {
  private constructor(
    public readonly client: BoxApiClient,
    private processHandle?: ChildProcess,
    public readonly configPath?: string,
    public readonly hostBinary?: string
  ) {}

  static async start(options: DaemonStartOptions = {}): Promise<Daemon> {
    const stateDir = options.stateDir ?? join(tmpdir(), `sagens-node-${process.pid}-${Date.now()}`);
    const configPath = options.userConfigPath ?? join(stateDir, "config.json");
    const hostBinary = resolveHostBinary(options.hostBinary);
    const config: UserConfig = {
      version: 1,
      adminUuid: randomUUID(),
      adminToken: randomBytes(32).toString("base64url"),
      endpoint: options.endpoint ?? (await allocateEndpoint())
    };
    await mkdir(stateDir, { recursive: true });
    await writeConfig(configPath, config);
    const child = spawnDaemon(hostBinary, stateDir, configPath, config);
    const client = await waitForDaemon(config, child, stateDir);
    return new Daemon(client, child, configPath, hostBinary);
  }

  static async connect(endpoint: string, adminUuid: string, adminToken: string): Promise<Daemon> {
    return new Daemon(await BoxApiClient.connect(endpoint, adminUuid, adminToken));
  }

  static async fromConfig(configPath: string): Promise<Daemon> {
    const raw = JSON.parse(await readFile(configPath, "utf8")) as Record<string, unknown>;
    const config = userConfigFromWire(raw);
    return new Daemon(await BoxApiClient.fromUserConfig(config), undefined, configPath);
  }

  async close(): Promise<void> {
    await this.quit();
    this.client.close();
  }

  async quit(): Promise<boolean> {
    if (!this.processHandle) {
      try {
        await this.client.shutdownDaemon();
        return true;
      } catch {
        return false;
      }
    }
    try {
      await this.client.shutdownDaemon();
    } catch {
      // The child may already be exiting; wait below still cleans it up.
    }
    await waitForExitOrKill(this.processHandle, 5000);
    this.processHandle = undefined;
    return true;
  }

  listBoxes(): Promise<BoxRecord[]> {
    return this.client.listBoxes();
  }

  async getBox(boxId: string): Promise<Box> {
    return new Box(this.client, await this.client.getBox(boxId));
  }

  async createBox(): Promise<Box> {
    return new Box(this.client, await this.client.createBox());
  }

  issueBoxCredentials(boxId: string): Promise<BoxCredentialBundle> {
    return this.client.issueBoxCredentials(boxId);
  }

  connectAsBox(boxId: string, boxToken?: string | null): Promise<BoxApiClient> {
    return BoxApiClient.connectAsBox(this.client.endpoint, boxId, boxToken);
  }

  adminAdd() {
    return this.client.adminAdd();
  }

  adminRemoveMe(): Promise<void> {
    return this.client.adminRemoveMe();
  }
}

async function writeConfig(path: string, config: UserConfig): Promise<void> {
  await mkdir(dirname(path), { recursive: true });
  const wire = {
    version: config.version,
    admin_uuid: config.adminUuid,
    admin_token: config.adminToken,
    endpoint: config.endpoint
  };
  await writeFile(path, `${JSON.stringify(wire, null, 2)}\n`, { mode: 0o600 });
  await chmod(path, 0o600);
}

function spawnDaemon(hostBinary: string, stateDir: string, configPath: string, config: UserConfig) {
  const logPath = join(stateDir, "daemon.log");
  const logFd = openSync(logPath, "a");
  try {
    return spawn(hostBinary, ["daemon"], {
      env: {
        ...process.env,
        SAGENS_STATE_DIR: stateDir,
        SAGENS_CONFIG: configPath,
        SAGENS_ENDPOINT: config.endpoint,
        SAGENS_BOOTSTRAP_ADMIN_UUID: config.adminUuid,
        SAGENS_BOOTSTRAP_ADMIN_TOKEN: config.adminToken
      },
      stdio: ["ignore", logFd, logFd]
    });
  } finally {
    closeSync(logFd);
  }
}

async function waitForDaemon(
  config: UserConfig,
  child: ChildProcess,
  stateDir: string
): Promise<BoxApiClient> {
  const deadline = Date.now() + 30_000;
  while (Date.now() < deadline) {
    if (child.exitCode !== null) {
      throw new SagensError(`daemon exited early (${child.exitCode}): ${await readLog(stateDir)}`);
    }
    try {
      const client = await BoxApiClient.fromUserConfig(config);
      await client.listBoxes();
      return client;
    } catch {
      await delay(100);
    }
  }
  throw new SagensError(`timed out waiting for daemon at ${config.endpoint}: ${await readLog(stateDir)}`);
}

async function waitForExitOrKill(child: ChildProcess, timeoutMs: number): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (child.exitCode !== null) {
      return;
    }
    await delay(100);
  }
  child.kill();
}

async function readLog(stateDir: string): Promise<string> {
  try {
    return await readFile(join(stateDir, "daemon.log"), "utf8");
  } catch {
    return "daemon log unavailable";
  }
}

function allocateEndpoint(): Promise<string> {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      server.close(() => {
        if (address && typeof address === "object") {
          resolve(`ws://127.0.0.1:${address.port}`);
        } else {
          reject(new SagensError("failed to allocate daemon port"));
        }
      });
    });
    server.on("error", reject);
  });
}
