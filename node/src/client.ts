import { Buffer } from "node:buffer";
import { mkdir, readdir, readFile, stat, writeFile } from "node:fs/promises";
import { basename, dirname, join, posix } from "node:path";

import {
  adminBundleFromWire,
  boxBundleFromWire,
  boxRecordFromWire,
  checkpointFromWire,
  completedExecution,
  execExitFromWire,
  fileNodeFromWire,
  objectValue,
  readFileFromWire,
  userConfigFromWire
} from "./decode.js";
import { SagensError } from "./errors.js";
import {
  AdminCredentialBundle,
  BoxCredentialBundle,
  BoxRecord,
  CheckpointRestoreMode,
  CompletedExecution,
  FileNode,
  ReadFileResult,
  UserConfig,
  WorkspaceCheckpointRecord
} from "./models.js";
import { BoxShell } from "./shell.js";
import { Transport, WireObject } from "./transport.js";

export class BoxApiClient {
  private constructor(private readonly transport: Transport) {}

  get endpoint(): string {
    return this.transport.endpoint;
  }

  static async connect(endpoint: string, adminUuid: string, adminToken: string): Promise<BoxApiClient> {
    const transport = await Transport.connect(
      endpoint,
      { type: "authenticate_admin", admin_uuid: adminUuid, admin_token: adminToken },
      { type: "admin", admin_uuid: adminUuid }
    );
    return new BoxApiClient(transport);
  }

  static async fromUserConfig(config: UserConfig | WireObject): Promise<BoxApiClient> {
    const userConfig = "adminUuid" in config ? (config as UserConfig) : userConfigFromWire(config);
    return BoxApiClient.connect(userConfig.endpoint, userConfig.adminUuid, userConfig.adminToken);
  }

  static async connectAsBox(
    endpoint: string,
    boxId: string,
    boxToken?: string | null
  ): Promise<BoxApiClient> {
    const transport = await Transport.connect(
      endpoint,
      { type: "authenticate_box", box_id: boxId, box_token: boxToken ?? null },
      { type: "box", box_id: boxId }
    );
    return new BoxApiClient(transport);
  }

  close(): void {
    this.transport.close();
  }

  async listBoxes(): Promise<BoxRecord[]> {
    const response = await this.request({ type: "list_boxes" }, "box_list");
    return (response.boxes as WireObject[]).map(boxRecordFromWire);
  }

  async getBox(boxId: string): Promise<BoxRecord> {
    const response = await this.request({ type: "get_box", box_id: boxId }, "box");
    return boxRecordFromWire(objectValue(response.record));
  }

  async createBox(): Promise<BoxRecord> {
    const response = await this.request({ type: "new_box" }, "box");
    return boxRecordFromWire(objectValue(response.record));
  }

  async startBox(boxId: string): Promise<BoxRecord> {
    const response = await this.request({ type: "start_box", box_id: boxId }, "box");
    return boxRecordFromWire(objectValue(response.record));
  }

  async stopBox(boxId: string): Promise<BoxRecord> {
    const response = await this.request({ type: "stop_box", box_id: boxId }, "box");
    return boxRecordFromWire(objectValue(response.record));
  }

  async removeBox(boxId: string): Promise<void> {
    await this.request({ type: "remove_box", box_id: boxId }, "box_removed");
  }

  async setBoxSetting(boxId: string, setting: string, value: boolean | number): Promise<BoxRecord> {
    const response = await this.request(
      { type: "set_box_setting", box_id: boxId, value: { setting: snakeCase(setting), value } },
      "box"
    );
    return boxRecordFromWire(objectValue(response.record));
  }

  async execBash(boxId: string, command: string): Promise<CompletedExecution> {
    return this.collectExec({
      type: "exec_bash",
      box_id: boxId,
      command,
      timeout_ms: null,
      kill_grace_ms: null
    });
  }

  async execPython(boxId: string, args: string[]): Promise<CompletedExecution> {
    return this.collectExec({
      type: "exec_python",
      box_id: boxId,
      args,
      timeout_ms: null,
      kill_grace_ms: null
    });
  }

  async execBashWithTimeout(
    boxId: string,
    command: string,
    timeoutMs: number,
    killGraceMs: number
  ): Promise<CompletedExecution> {
    return this.collectExec({
      type: "exec_bash",
      box_id: boxId,
      command,
      timeout_ms: timeoutMs,
      kill_grace_ms: killGraceMs
    });
  }

  async openBash(boxId: string): Promise<BoxShell> {
    return this.openShell(boxId, "bash");
  }

  async openPython(boxId: string): Promise<BoxShell> {
    return this.openShell(boxId, "python");
  }

  async listFiles(boxId: string, path = "/workspace"): Promise<FileNode[]> {
    const response = await this.request({ type: "fs_list", box_id: boxId, path }, "files");
    return (response.entries as WireObject[]).map(fileNodeFromWire);
  }

  async readFile(boxId: string, path: string, limit = 16 * 1024 * 1024): Promise<ReadFileResult> {
    const response = await this.request({ type: "fs_read", box_id: boxId, path, limit }, "file");
    return readFileFromWire(objectValue(response.file));
  }

  async writeFile(boxId: string, path: string, data: Buffer | string, createParents = true): Promise<void> {
    const payload = Buffer.isBuffer(data) ? data : Buffer.from(data);
    await this.request(
      {
        type: "fs_write",
        box_id: boxId,
        path,
        data: payload.toString("base64"),
        create_parents: createParents
      },
      "ack"
    );
  }

  async makeDir(boxId: string, path: string, recursive = true): Promise<void> {
    await this.request({ type: "fs_mkdir", box_id: boxId, path, recursive }, "ack");
  }

  async removePath(boxId: string, path: string, recursive = false): Promise<void> {
    await this.request({ type: "fs_remove", box_id: boxId, path, recursive }, "ack");
  }

  async checkpointCreate(
    boxId: string,
    name?: string | null,
    metadata: Record<string, string> = {}
  ): Promise<WorkspaceCheckpointRecord> {
    const response = await this.request(
      { type: "checkpoint_create", box_id: boxId, name: name ?? null, metadata },
      "checkpoint"
    );
    return checkpointFromWire(objectValue(response.checkpoint));
  }

  async checkpointList(boxId: string): Promise<WorkspaceCheckpointRecord[]> {
    const response = await this.request({ type: "checkpoint_list", box_id: boxId }, "checkpoint_list");
    return (response.checkpoints as WireObject[]).map(checkpointFromWire);
  }

  async checkpointRestore(
    boxId: string,
    checkpointId: string,
    mode: CheckpointRestoreMode = "rollback"
  ): Promise<WorkspaceCheckpointRecord> {
    const response = await this.request(
      { type: "checkpoint_restore", box_id: boxId, checkpoint_id: checkpointId, mode },
      "checkpoint"
    );
    return checkpointFromWire(objectValue(response.checkpoint));
  }

  async checkpointFork(
    boxId: string,
    checkpointId: string,
    newBoxName?: string | null
  ): Promise<BoxRecord> {
    const response = await this.request(
      { type: "checkpoint_fork", box_id: boxId, checkpoint_id: checkpointId, new_box_name: newBoxName ?? null },
      "box"
    );
    return boxRecordFromWire(objectValue(response.record));
  }

  async checkpointDelete(boxId: string, checkpointId: string): Promise<void> {
    await this.request(
      { type: "checkpoint_delete", box_id: boxId, checkpoint_id: checkpointId },
      "ack"
    );
  }

  async shutdownDaemon(): Promise<void> {
    await this.request({ type: "shutdown_daemon" }, "ack");
  }

  async adminAdd(): Promise<AdminCredentialBundle> {
    const response = await this.request({ type: "admin_add" }, "admin_added");
    return adminBundleFromWire(objectValue(response.bundle));
  }

  async issueBoxCredentials(boxId: string): Promise<BoxCredentialBundle> {
    const response = await this.request(
      { type: "box_issue_credentials", box_id: boxId },
      "box_credentials"
    );
    return boxBundleFromWire(objectValue(response.bundle));
  }

  async adminRemoveMe(): Promise<void> {
    await this.request({ type: "admin_remove_me" }, "ack");
  }

  async uploadPath(boxId: string, localPath: string, remotePath: string): Promise<void> {
    const localStat = await stat(localPath);
    if (localStat.isDirectory()) {
      await this.makeDir(boxId, remotePath, true);
      for (const item of await readdir(localPath)) {
        await this.uploadPath(boxId, join(localPath, item), posix.join(remotePath, item));
      }
      return;
    }
    await this.writeFile(boxId, remotePath, await readFile(localPath), true);
  }

  async downloadPath(boxId: string, remotePath: string, localPath: string): Promise<void> {
    const file = await this.readFile(boxId, remotePath);
    const destination = localPath.endsWith("/") ? join(localPath, basename(remotePath)) : localPath;
    await mkdir(dirname(destination), { recursive: true });
    await writeFile(destination, file.data);
  }

  private async collectExec(request: WireObject): Promise<CompletedExecution> {
    request.request_id = this.transport.nextRequestId();
    const events = this.transport.openExecStream(request);
    const stdout: Buffer[] = [];
    const stderr: Buffer[] = [];
    const output: Buffer[] = [];
    while (true) {
      const event = await events.next();
      if (event.type === "exec_output") {
        const data = Buffer.from(String(event.data), "base64");
        output.push(data);
        if (event.stream === "stdout") {
          stdout.push(data);
        } else {
          stderr.push(data);
        }
        continue;
      }
      return completedExecution(
        execExitFromWire(event.status),
        Buffer.concat(output),
        Buffer.concat(stdout),
        Buffer.concat(stderr)
      );
    }
  }

  private async openShell(boxId: string, target: "bash" | "python"): Promise<BoxShell> {
    const response = await this.request({ type: "open_shell", box_id: boxId, target }, "shell_opened");
    const shellId = String(response.shell_id);
    return new BoxShell(shellId, this.transport, this.transport.registerShell(shellId));
  }

  private async request(request: WireObject, expectedType: string): Promise<WireObject> {
    request.request_id = this.transport.nextRequestId();
    return this.transport.requestResponse(request, expectedType);
  }
}

function snakeCase(name: string): string {
  return name.trim().replaceAll("-", "_");
}
