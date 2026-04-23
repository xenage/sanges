from __future__ import annotations

from dataclasses import dataclass, field
from enum import Enum
from pathlib import Path
from uuid import UUID


class BoxStatus(str, Enum):
    CREATED = "created"
    RUNNING = "running"
    STOPPED = "stopped"
    FAILED = "failed"
    REMOVING = "removing"


class FileKind(str, Enum):
    FILE = "file"
    DIRECTORY = "directory"
    SYMLINK = "symlink"


class WorkspaceChangeKind(str, Enum):
    ADDED = "added"
    MODIFIED = "modified"
    DELETED = "deleted"
    TYPE_CHANGED = "type_changed"


class CheckpointRestoreMode(str, Enum):
    ROLLBACK = "rollback"
    REPLACE = "replace"


@dataclass(frozen=True)
class ExecExit:
    kind: str
    code: int | None = None

    @property
    def success(self) -> bool:
        return self.kind == "success"


@dataclass(frozen=True)
class UserConfig:
    version: int
    admin_uuid: UUID
    admin_token: str
    endpoint: str


@dataclass(frozen=True)
class AdminCredentialBundle:
    admin_uuid: UUID
    admin_token: str
    endpoint: str


@dataclass(frozen=True)
class BoxCredentialBundle:
    box_id: UUID
    box_token: str
    endpoint: str


@dataclass(frozen=True)
class BoxNumericSetting:
    current: int
    max: int


@dataclass(frozen=True)
class BoxBooleanSetting:
    current: bool
    max: bool


@dataclass(frozen=True)
class BoxSettings:
    cpu_cores: BoxNumericSetting
    memory_mb: BoxNumericSetting
    fs_size_mib: BoxNumericSetting
    max_processes: BoxNumericSetting
    network_enabled: BoxBooleanSetting


@dataclass(frozen=True)
class BoxRuntimeUsage:
    cpu_millicores: int
    memory_used_mib: int
    fs_used_mib: int
    process_count: int


@dataclass(frozen=True)
class BoxRecord:
    box_id: UUID
    name: str | None
    status: BoxStatus
    settings: BoxSettings | None
    runtime_usage: BoxRuntimeUsage | None
    workspace_path: Path
    active_sandbox_id: UUID | None
    created_at_ms: int
    last_start_at_ms: int | None
    last_stop_at_ms: int | None
    last_error: str | None


@dataclass(frozen=True)
class CompletedExecution:
    exit_status: ExecExit
    exit_code: int | None
    output: bytes
    stdout: bytes
    stderr: bytes


@dataclass(frozen=True)
class FileNode:
    path: str
    kind: FileKind
    size: int
    digest: str | None
    target: str | None


@dataclass(frozen=True)
class ReadFileResult:
    path: str
    data: bytes
    truncated: bool


@dataclass(frozen=True)
class WorkspaceChange:
    path: str
    kind: WorkspaceChangeKind
    kind_after: FileKind | None


@dataclass(frozen=True)
class WorkspaceCheckpointSummary:
    checkpoint_id: str
    workspace_id: str
    name: str | None
    metadata: dict[str, str] = field(default_factory=dict)
    created_at_ms: int = 0


@dataclass(frozen=True)
class WorkspaceCheckpointRecord:
    summary: WorkspaceCheckpointSummary
    source_checkpoint_id: str | None
    changes: list[WorkspaceChange]


@dataclass(frozen=True)
class ManagedDaemonPaths:
    state_dir: Path
    user_config_path: Path
    endpoint: str
    pid_path: Path


@dataclass(frozen=True)
class ManagedDaemonStartInfo:
    paths: ManagedDaemonPaths
    user_config: UserConfig
    already_running: bool
