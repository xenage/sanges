import {
  AdminCredentialBundle,
  BoxCredentialBundle,
  BoxRecord,
  BoxSettings,
  BoxRuntimeUsage,
  CompletedExecution,
  ExecExit,
  FileNode,
  ReadFileResult,
  UserConfig,
  WorkspaceCheckpointRecord
} from "./models.js";

export function userConfigFromWire(raw: Record<string, unknown>): UserConfig {
  return {
    version: numberValue(raw.version),
    adminUuid: stringValue(raw.admin_uuid),
    adminToken: stringValue(raw.admin_token),
    endpoint: stringValue(raw.endpoint)
  };
}

export function adminBundleFromWire(raw: Record<string, unknown>): AdminCredentialBundle {
  return {
    adminUuid: stringValue(raw.admin_uuid),
    adminToken: stringValue(raw.admin_token),
    endpoint: stringValue(raw.endpoint)
  };
}

export function boxBundleFromWire(raw: Record<string, unknown>): BoxCredentialBundle {
  return {
    boxId: stringValue(raw.box_id),
    boxToken: stringValue(raw.box_token),
    endpoint: stringValue(raw.endpoint)
  };
}

export function boxRecordFromWire(raw: Record<string, unknown>): BoxRecord {
  return {
    boxId: stringValue(raw.box_id),
    name: optionalString(raw.name),
    status: stringValue(raw.status) as BoxRecord["status"],
    settings: raw.settings ? boxSettingsFromWire(objectValue(raw.settings)) : null,
    runtimeUsage: raw.runtime_usage
      ? runtimeUsageFromWire(objectValue(raw.runtime_usage))
      : null,
    workspacePath: stringValue(raw.workspace_path),
    activeSandboxId: optionalString(raw.active_sandbox_id),
    createdAtMs: numberValue(raw.created_at_ms),
    lastStartAtMs: optionalNumber(raw.last_start_at_ms),
    lastStopAtMs: optionalNumber(raw.last_stop_at_ms),
    lastError: optionalString(raw.last_error)
  };
}

export function execExitFromWire(raw: unknown): ExecExit {
  if (typeof raw === "string") {
    return { kind: raw, success: raw === "success" };
  }
  const obj = objectValue(raw);
  if (typeof obj.exit_code === "number") {
    return { kind: "exit_code", code: obj.exit_code, success: obj.exit_code === 0 };
  }
  const [kind, value] = Object.entries(obj)[0] ?? ["killed", null];
  const code = typeof value === "number" ? value : null;
  return { kind, code, success: kind === "success" || code === 0 };
}

export function completedExecution(
  exitStatus: ExecExit,
  output: Buffer,
  stdout: Buffer,
  stderr: Buffer
): CompletedExecution {
  const exitCode =
    exitStatus.kind === "success" ? 0 : exitStatus.kind === "exit_code" ? exitStatus.code : null;
  return {
    exitStatus,
    exitCode,
    output,
    stdout,
    stderr,
    outputText: output.toString(),
    stdoutText: stdout.toString(),
    stderrText: stderr.toString()
  };
}

export function fileNodeFromWire(raw: Record<string, unknown>): FileNode {
  return {
    path: stringValue(raw.path),
    kind: stringValue(raw.kind) as FileNode["kind"],
    size: numberValue(raw.size),
    digest: optionalString(raw.digest),
    target: optionalString(raw.target)
  };
}

export function readFileFromWire(raw: Record<string, unknown>): ReadFileResult {
  return {
    path: stringValue(raw.path),
    data: Buffer.from(arrayValue(raw.data) as number[]),
    truncated: booleanValue(raw.truncated)
  };
}

export function checkpointFromWire(raw: Record<string, unknown>): WorkspaceCheckpointRecord {
  const summary = objectValue(raw.summary);
  return {
    summary: {
      checkpointId: stringValue(summary.checkpoint_id),
      workspaceId: stringValue(summary.workspace_id),
      name: optionalString(summary.name),
      metadata: recordString(summary.metadata),
      createdAtMs: numberValue(summary.created_at_ms)
    },
    sourceCheckpointId: optionalString(raw.source_checkpoint_id),
    changes: arrayValue(raw.changes).map((item) => {
      const change = objectValue(item);
      return {
        path: stringValue(change.path),
        kind: stringValue(change.kind) as WorkspaceCheckpointRecord["changes"][number]["kind"],
        kindAfter: optionalString(change.kind_after) as
          | WorkspaceCheckpointRecord["changes"][number]["kindAfter"]
          | undefined
      };
    })
  };
}

export function objectValue(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new TypeError("expected object value");
  }
  return value as Record<string, unknown>;
}

function boxSettingsFromWire(raw: Record<string, unknown>): BoxSettings {
  return {
    cpuCores: settingNumber(raw.cpu_cores),
    memoryMb: settingNumber(raw.memory_mb),
    fsSizeMib: settingNumber(raw.fs_size_mib),
    maxProcesses: settingNumber(raw.max_processes),
    networkEnabled: settingBoolean(raw.network_enabled)
  };
}

function runtimeUsageFromWire(raw: Record<string, unknown>): BoxRuntimeUsage {
  return {
    cpuMillicores: numberValue(raw.cpu_millicores),
    memoryUsedMib: numberValue(raw.memory_used_mib),
    fsUsedMib: numberValue(raw.fs_used_mib),
    processCount: numberValue(raw.process_count)
  };
}

function settingNumber(value: unknown) {
  const raw = objectValue(value);
  return { current: numberValue(raw.current), max: numberValue(raw.max) };
}

function settingBoolean(value: unknown) {
  const raw = objectValue(value);
  return { current: booleanValue(raw.current), max: booleanValue(raw.max) };
}

function recordString(value: unknown): Record<string, string> {
  if (!value) {
    return {};
  }
  const raw = objectValue(value);
  return Object.fromEntries(Object.entries(raw).map(([key, item]) => [key, stringValue(item)]));
}

function arrayValue(value: unknown): unknown[] {
  if (!Array.isArray(value)) {
    throw new TypeError("expected array value");
  }
  return value;
}

function booleanValue(value: unknown): boolean {
  if (typeof value !== "boolean") {
    throw new TypeError("expected boolean value");
  }
  return value;
}

function numberValue(value: unknown): number {
  if (typeof value !== "number") {
    throw new TypeError("expected number value");
  }
  return value;
}

function stringValue(value: unknown): string {
  if (typeof value !== "string") {
    throw new TypeError("expected string value");
  }
  return value;
}

function optionalNumber(value: unknown): number | null {
  return value === undefined || value === null ? null : numberValue(value);
}

function optionalString(value: unknown): string | null {
  return value === undefined || value === null ? null : stringValue(value);
}
