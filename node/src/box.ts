import { Buffer } from "node:buffer";

import { BoxApiClient } from "./client.js";
import {
  BoxRecord,
  CheckpointRestoreMode,
  CompletedExecution,
  FileNode,
  ReadFileResult,
  WorkspaceCheckpointRecord
} from "./models.js";
import { BoxShell } from "./shell.js";

export class BoxFs {
  constructor(
    private readonly client: BoxApiClient,
    private readonly boxId: string
  ) {}

  list(path = "/workspace"): Promise<FileNode[]> {
    return this.client.listFiles(this.boxId, path);
  }

  read(path: string, limit = 16 * 1024 * 1024): Promise<ReadFileResult> {
    return this.client.readFile(this.boxId, path, limit);
  }

  write(path: string, data: Buffer | string, createParents = true): Promise<void> {
    return this.client.writeFile(this.boxId, path, data, createParents);
  }

  mkdir(path: string, recursive = true): Promise<void> {
    return this.client.makeDir(this.boxId, path, recursive);
  }

  remove(path: string, recursive = false): Promise<void> {
    return this.client.removePath(this.boxId, path, recursive);
  }

  upload(localPath: string, remotePath: string): Promise<void> {
    return this.client.uploadPath(this.boxId, localPath, remotePath);
  }

  download(remotePath: string, localPath: string): Promise<void> {
    return this.client.downloadPath(this.boxId, remotePath, localPath);
  }
}

export class BoxCheckpoint {
  constructor(
    private readonly client: BoxApiClient,
    private readonly boxId: string
  ) {}

  create(
    name?: string | null,
    metadata: Record<string, string> = {}
  ): Promise<WorkspaceCheckpointRecord> {
    return this.client.checkpointCreate(this.boxId, name, metadata);
  }

  list(): Promise<WorkspaceCheckpointRecord[]> {
    return this.client.checkpointList(this.boxId);
  }

  restore(
    checkpointId: string,
    mode: CheckpointRestoreMode = "rollback"
  ): Promise<WorkspaceCheckpointRecord> {
    return this.client.checkpointRestore(this.boxId, checkpointId, mode);
  }

  fork(checkpointId: string, newBoxName?: string | null): Promise<BoxRecord> {
    return this.client.checkpointFork(this.boxId, checkpointId, newBoxName);
  }

  delete(checkpointId: string): Promise<void> {
    return this.client.checkpointDelete(this.boxId, checkpointId);
  }
}

export class Box {
  readonly fs: BoxFs;
  readonly checkpoint: BoxCheckpoint;

  constructor(
    private readonly client: BoxApiClient,
    public record: BoxRecord
  ) {
    this.fs = new BoxFs(client, record.boxId);
    this.checkpoint = new BoxCheckpoint(client, record.boxId);
  }

  get boxId(): string {
    return this.record.boxId;
  }

  async refresh(): Promise<BoxRecord> {
    this.record = await this.client.getBox(this.boxId);
    return this.record;
  }

  async start(): Promise<BoxRecord> {
    this.record = await this.client.startBox(this.boxId);
    return this.record;
  }

  async stop(): Promise<BoxRecord> {
    this.record = await this.client.stopBox(this.boxId);
    return this.record;
  }

  remove(): Promise<void> {
    return this.client.removeBox(this.boxId);
  }

  async set(setting: string, value: boolean | number): Promise<BoxRecord> {
    this.record = await this.client.setBoxSetting(this.boxId, setting, value);
    return this.record;
  }

  execBash(command: string): Promise<CompletedExecution> {
    return this.client.execBash(this.boxId, command);
  }

  execPython(args: string[]): Promise<CompletedExecution> {
    return this.client.execPython(this.boxId, args);
  }

  execBashWithTimeout(
    command: string,
    timeoutMs: number,
    killGraceMs: number
  ): Promise<CompletedExecution> {
    return this.client.execBashWithTimeout(this.boxId, command, timeoutMs, killGraceMs);
  }

  openBash(): Promise<BoxShell> {
    return this.client.openBash(this.boxId);
  }

  openPython(): Promise<BoxShell> {
    return this.client.openPython(this.boxId);
  }
}
