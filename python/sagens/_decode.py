from __future__ import annotations

from pathlib import Path
from uuid import UUID

from ._models import (
    AdminCredentialBundle,
    BoxBooleanSetting,
    BoxCredentialBundle,
    BoxNumericSetting,
    BoxRecord,
    BoxRuntimeUsage,
    BoxSettings,
    BoxStatus,
    CheckpointRestoreMode,
    ExecExit,
    FileKind,
    FileNode,
    ManagedDaemonPaths,
    ManagedDaemonStartInfo,
    ReadFileResult,
    UserConfig,
    WorkspaceChange,
    WorkspaceChangeKind,
    WorkspaceCheckpointRecord,
    WorkspaceCheckpointSummary,
)


def user_config_from_dict(raw: dict) -> UserConfig:
    return UserConfig(
        version=raw["version"],
        admin_uuid=UUID(raw["admin_uuid"]),
        admin_token=raw["admin_token"],
        endpoint=raw["endpoint"],
    )


def admin_bundle_from_dict(raw: dict) -> AdminCredentialBundle:
    return AdminCredentialBundle(
        admin_uuid=UUID(raw["admin_uuid"]),
        admin_token=raw["admin_token"],
        endpoint=raw["endpoint"],
    )


def box_bundle_from_dict(raw: dict) -> BoxCredentialBundle:
    return BoxCredentialBundle(
        box_id=UUID(raw["box_id"]),
        box_token=raw["box_token"],
        endpoint=raw["endpoint"],
    )


def box_record_from_dict(raw: dict) -> BoxRecord:
    sandbox_id = raw.get("active_sandbox_id")
    return BoxRecord(
        box_id=UUID(raw["box_id"]),
        name=raw.get("name"),
        status=BoxStatus(raw["status"]),
        settings=box_settings_from_dict(raw["settings"]) if raw.get("settings") else None,
        runtime_usage=(
            runtime_usage_from_dict(raw["runtime_usage"])
            if raw.get("runtime_usage")
            else None
        ),
        workspace_path=Path(raw["workspace_path"]),
        active_sandbox_id=UUID(sandbox_id) if sandbox_id else None,
        created_at_ms=raw["created_at_ms"],
        last_start_at_ms=raw.get("last_start_at_ms"),
        last_stop_at_ms=raw.get("last_stop_at_ms"),
        last_error=raw.get("last_error"),
    )


def box_settings_from_dict(raw: dict) -> BoxSettings:
    return BoxSettings(
        cpu_cores=numeric_setting_from_dict(raw["cpu_cores"]),
        memory_mb=numeric_setting_from_dict(raw["memory_mb"]),
        fs_size_mib=numeric_setting_from_dict(raw["fs_size_mib"]),
        max_processes=numeric_setting_from_dict(raw["max_processes"]),
        network_enabled=boolean_setting_from_dict(raw["network_enabled"]),
    )


def runtime_usage_from_dict(raw: dict) -> BoxRuntimeUsage:
    return BoxRuntimeUsage(
        cpu_millicores=raw["cpu_millicores"],
        memory_used_mib=raw["memory_used_mib"],
        fs_used_mib=raw["fs_used_mib"],
        process_count=raw["process_count"],
    )


def numeric_setting_from_dict(raw: dict) -> BoxNumericSetting:
    return BoxNumericSetting(current=raw["current"], max=raw["max"])


def boolean_setting_from_dict(raw: dict) -> BoxBooleanSetting:
    return BoxBooleanSetting(current=raw["current"], max=raw["max"])


def exec_exit_from_raw(raw: str | dict) -> ExecExit:
    if isinstance(raw, str):
        return ExecExit(kind=raw)
    if "exit_code" in raw:
        return ExecExit(kind="exit_code", code=raw["exit_code"])
    kind, value = next(iter(raw.items()))
    return ExecExit(kind=kind, code=value if isinstance(value, int) else None)


def file_node_from_dict(raw: dict) -> FileNode:
    return FileNode(
        path=raw["path"],
        kind=FileKind(raw["kind"]),
        size=raw["size"],
        digest=raw.get("digest"),
        target=raw.get("target"),
    )


def read_file_from_dict(raw: dict) -> ReadFileResult:
    return ReadFileResult(
        path=raw["path"],
        data=bytes(raw["data"]),
        truncated=raw["truncated"],
    )


def workspace_change_from_dict(raw: dict) -> WorkspaceChange:
    kind_after = raw.get("kind_after")
    return WorkspaceChange(
        path=raw["path"],
        kind=WorkspaceChangeKind(raw["kind"]),
        kind_after=FileKind(kind_after) if kind_after else None,
    )


def checkpoint_record_from_dict(raw: dict) -> WorkspaceCheckpointRecord:
    return WorkspaceCheckpointRecord(
        summary=checkpoint_summary_from_dict(raw["summary"]),
        source_checkpoint_id=raw.get("source_checkpoint_id"),
        changes=[workspace_change_from_dict(item) for item in raw["changes"]],
    )


def checkpoint_summary_from_dict(raw: dict) -> WorkspaceCheckpointSummary:
    return WorkspaceCheckpointSummary(
        checkpoint_id=raw["checkpoint_id"],
        workspace_id=raw["workspace_id"],
        name=raw.get("name"),
        metadata=dict(raw.get("metadata", {})),
        created_at_ms=raw["created_at_ms"],
    )


def daemon_start_info_from_dict(raw: dict) -> ManagedDaemonStartInfo:
    return ManagedDaemonStartInfo(
        paths=ManagedDaemonPaths(
            state_dir=Path(raw["paths"]["state_dir"]),
            user_config_path=Path(raw["paths"]["user_config_path"]),
            endpoint=raw["paths"]["endpoint"],
            pid_path=Path(raw["paths"]["pid_path"]),
        ),
        user_config=user_config_from_dict(raw["user_config"]),
        already_running=raw["already_running"],
    )


def serialize_box_setting(name: str, value: bool | int) -> dict:
    setting_name = snake_case(name)
    return {"setting": setting_name, "value": value}


def serialize_restore_mode(mode: CheckpointRestoreMode | str) -> str:
    return mode.value if isinstance(mode, CheckpointRestoreMode) else str(mode)


def snake_case(name: str) -> str:
    return name.strip().replace("-", "_")
