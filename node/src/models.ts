export type BoxStatus = "created" | "running" | "stopped" | "failed" | "removing";
export type FileKind = "file" | "directory" | "symlink";
export type WorkspaceChangeKind = "added" | "modified" | "deleted" | "type_changed";
export type CheckpointRestoreMode = "rollback" | "replace";

export interface UserConfig {
  version: number;
  adminUuid: string;
  adminToken: string;
  endpoint: string;
}

export interface AdminCredentialBundle {
  adminUuid: string;
  adminToken: string;
  endpoint: string;
}

export interface BoxCredentialBundle {
  boxId: string;
  boxToken: string;
  endpoint: string;
}

export interface BoxSetting<T> {
  current: T;
  max: T;
}

export interface BoxSettings {
  cpuCores: BoxSetting<number>;
  memoryMb: BoxSetting<number>;
  fsSizeMib: BoxSetting<number>;
  maxProcesses: BoxSetting<number>;
  networkEnabled: BoxSetting<boolean>;
}

export interface BoxRuntimeUsage {
  cpuMillicores: number;
  memoryUsedMib: number;
  fsUsedMib: number;
  processCount: number;
}

export interface BoxRecord {
  boxId: string;
  name?: string | null;
  status: BoxStatus;
  settings?: BoxSettings | null;
  runtimeUsage?: BoxRuntimeUsage | null;
  workspacePath: string;
  activeSandboxId?: string | null;
  createdAtMs: number;
  lastStartAtMs?: number | null;
  lastStopAtMs?: number | null;
  lastError?: string | null;
}

export interface ExecExit {
  kind: string;
  code?: number | null;
  success: boolean;
}

export interface CompletedExecution {
  exitStatus: ExecExit;
  exitCode?: number | null;
  output: Buffer;
  stdout: Buffer;
  stderr: Buffer;
  outputText: string;
  stdoutText: string;
  stderrText: string;
}

export interface FileNode {
  path: string;
  kind: FileKind;
  size: number;
  digest?: string | null;
  target?: string | null;
}

export interface ReadFileResult {
  path: string;
  data: Buffer;
  truncated: boolean;
}

export interface WorkspaceChange {
  path: string;
  kind: WorkspaceChangeKind;
  kindAfter?: FileKind | null;
}

export interface WorkspaceCheckpointSummary {
  checkpointId: string;
  workspaceId: string;
  name?: string | null;
  metadata: Record<string, string>;
  createdAtMs: number;
}

export interface WorkspaceCheckpointRecord {
  summary: WorkspaceCheckpointSummary;
  sourceCheckpointId?: string | null;
  changes: WorkspaceChange[];
}
