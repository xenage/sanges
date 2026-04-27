from __future__ import annotations

import base64
from pathlib import Path
from typing import Any
from uuid import UUID

from ._decode import (
    admin_bundle_from_dict,
    box_bundle_from_dict,
    box_record_from_dict,
    checkpoint_record_from_dict,
    exec_exit_from_raw,
    file_node_from_dict,
    read_file_from_dict,
    serialize_box_setting,
    serialize_restore_mode,
    user_config_from_dict,
)
from ._download import resolve_download_file_path, should_fallback_to_file_download
from ._errors import SagensError
from ._models import (
    AdminCredentialBundle,
    BoxCredentialBundle,
    BoxRecord,
    CheckpointRestoreMode,
    CompletedExecution,
    ReadFileResult,
    UserConfig,
    WorkspaceCheckpointRecord,
)
from ._shell import BoxShell
from ._transport import _Transport


class BoxApiClient:
    def __init__(self, transport: _Transport) -> None:
        self._transport = transport

    @property
    def endpoint(self) -> str:
        return self._transport.endpoint

    @classmethod
    def connect(cls, endpoint: str, admin_uuid: UUID | str, admin_token: str) -> "BoxApiClient":
        admin_uuid = UUID(str(admin_uuid))
        return cls(
            _Transport(
                endpoint,
                {
                    "type": "authenticate_admin",
                    "admin_uuid": str(admin_uuid),
                    "admin_token": admin_token,
                },
                {"type": "admin", "admin_uuid": str(admin_uuid)},
            )
        )

    @classmethod
    def from_user_config(cls, config: UserConfig | dict[str, Any]) -> "BoxApiClient":
        user_config = user_config_from_dict(config) if isinstance(config, dict) else config
        return cls.connect(user_config.endpoint, user_config.admin_uuid, user_config.admin_token)

    @classmethod
    def connect_as_box(
        cls,
        endpoint: str,
        box_id: UUID | str,
        box_token: str | None,
    ) -> "BoxApiClient":
        box_id = UUID(str(box_id))
        return cls(
            _Transport(
                endpoint,
                {
                    "type": "authenticate_box",
                    "box_id": str(box_id),
                    "box_token": box_token,
                },
                {"type": "box", "box_id": str(box_id)},
            )
        )

    def close(self) -> None:
        self._transport.close()

    def list_boxes(self) -> list[BoxRecord]:
        response = self._request({"type": "list_boxes"}, "box_list")
        return [box_record_from_dict(item) for item in response["boxes"]]

    def get_box(self, box_id: UUID | str) -> BoxRecord:
        response = self._request({"type": "get_box", "box_id": str(box_id)}, "box")
        return box_record_from_dict(response["record"])

    def create_box(self) -> BoxRecord:
        response = self._request({"type": "new_box"}, "box")
        return box_record_from_dict(response["record"])

    def start_box(self, box_id: UUID | str) -> BoxRecord:
        response = self._request({"type": "start_box", "box_id": str(box_id)}, "box")
        return box_record_from_dict(response["record"])

    def stop_box(self, box_id: UUID | str) -> BoxRecord:
        response = self._request({"type": "stop_box", "box_id": str(box_id)}, "box")
        return box_record_from_dict(response["record"])

    def remove_box(self, box_id: UUID | str) -> None:
        self._request({"type": "remove_box", "box_id": str(box_id)}, "box_removed")

    def set_box_setting(self, box_id: UUID | str, setting: str, value: bool | int) -> BoxRecord:
        response = self._request(
            {
                "type": "set_box_setting",
                "box_id": str(box_id),
                "value": serialize_box_setting(setting, value),
            },
            "box",
        )
        return box_record_from_dict(response["record"])

    def exec_bash(self, box_id: UUID | str, command: str) -> CompletedExecution:
        return self._collect_exec(
            {
                "type": "exec_bash",
                "box_id": str(box_id),
                "command": command,
                "timeout_ms": None,
                "kill_grace_ms": None,
            }
        )

    def exec_python(self, box_id: UUID | str, args: list[str]) -> CompletedExecution:
        return self._collect_exec(
            {
                "type": "exec_python",
                "box_id": str(box_id),
                "args": args,
                "timeout_ms": None,
                "kill_grace_ms": None,
            }
        )

    def exec_bash_with_timeout(
        self,
        box_id: UUID | str,
        command: str,
        timeout_ms: int,
        kill_grace_ms: int,
    ) -> CompletedExecution:
        return self._collect_exec(
            {
                "type": "exec_bash",
                "box_id": str(box_id),
                "command": command,
                "timeout_ms": timeout_ms,
                "kill_grace_ms": kill_grace_ms,
            }
        )

    def open_bash(self, box_id: UUID | str) -> BoxShell:
        return self._open_shell(box_id, "bash")

    def open_python(self, box_id: UUID | str) -> BoxShell:
        return self._open_shell(box_id, "python")

    def list_files(self, box_id: UUID | str, path: str) -> list:
        response = self._request(
            {"type": "fs_list", "box_id": str(box_id), "path": path},
            "files",
        )
        return [file_node_from_dict(item) for item in response["entries"]]

    def read_file(self, box_id: UUID | str, path: str, limit: int) -> ReadFileResult:
        response = self._request(
            {"type": "fs_read", "box_id": str(box_id), "path": path, "limit": limit},
            "file",
        )
        return read_file_from_dict(response["file"])

    def write_file(
        self,
        box_id: UUID | str,
        path: str,
        data: bytes,
        create_parents: bool = True,
    ) -> None:
        self._request(
            {
                "type": "fs_write",
                "box_id": str(box_id),
                "path": path,
                "data": base64.b64encode(data).decode(),
                "create_parents": create_parents,
            },
            "ack",
        )

    def make_dir(self, box_id: UUID | str, path: str, recursive: bool = True) -> None:
        self._request(
            {
                "type": "fs_mkdir",
                "box_id": str(box_id),
                "path": path,
                "recursive": recursive,
            },
            "ack",
        )

    def remove_path(self, box_id: UUID | str, path: str, recursive: bool = False) -> None:
        self._request(
            {
                "type": "fs_remove",
                "box_id": str(box_id),
                "path": path,
                "recursive": recursive,
            },
            "ack",
        )

    def checkpoint_create(
        self,
        box_id: UUID | str,
        name: str | None = None,
        metadata: dict[str, str] | None = None,
    ) -> WorkspaceCheckpointRecord:
        response = self._request(
            {
                "type": "checkpoint_create",
                "box_id": str(box_id),
                "name": name,
                "metadata": dict(metadata or {}),
            },
            "checkpoint",
        )
        return checkpoint_record_from_dict(response["checkpoint"])

    def checkpoint_list(self, box_id: UUID | str) -> list[WorkspaceCheckpointRecord]:
        response = self._request(
            {"type": "checkpoint_list", "box_id": str(box_id)},
            "checkpoint_list",
        )
        return [checkpoint_record_from_dict(item) for item in response["checkpoints"]]

    def checkpoint_restore(
        self,
        box_id: UUID | str,
        checkpoint_id: str,
        mode: CheckpointRestoreMode | str = CheckpointRestoreMode.ROLLBACK,
    ) -> WorkspaceCheckpointRecord:
        response = self._request(
            {
                "type": "checkpoint_restore",
                "box_id": str(box_id),
                "checkpoint_id": checkpoint_id,
                "mode": serialize_restore_mode(mode),
            },
            "checkpoint",
        )
        return checkpoint_record_from_dict(response["checkpoint"])

    def checkpoint_fork(
        self,
        box_id: UUID | str,
        checkpoint_id: str,
        new_box_name: str | None = None,
    ) -> BoxRecord:
        response = self._request(
            {
                "type": "checkpoint_fork",
                "box_id": str(box_id),
                "checkpoint_id": checkpoint_id,
                "new_box_name": new_box_name,
            },
            "box",
        )
        return box_record_from_dict(response["record"])

    def checkpoint_delete(self, box_id: UUID | str, checkpoint_id: str) -> None:
        self._request(
            {
                "type": "checkpoint_delete",
                "box_id": str(box_id),
                "checkpoint_id": checkpoint_id,
            },
            "ack",
        )

    def shutdown_daemon(self) -> None:
        self._request({"type": "shutdown_daemon"}, "ack")

    def admin_add(self) -> AdminCredentialBundle:
        response = self._request({"type": "admin_add"}, "admin_added")
        return admin_bundle_from_dict(response["bundle"])

    def issue_box_credentials(self, box_id: UUID | str) -> BoxCredentialBundle:
        response = self._request(
            {"type": "box_issue_credentials", "box_id": str(box_id)},
            "box_credentials",
        )
        return box_bundle_from_dict(response["bundle"])

    def admin_remove_me(self) -> None:
        self._request({"type": "admin_remove_me"}, "ack")

    def upload_path(self, box_id: UUID | str, local_path: str | Path, remote_path: str | Path) -> None:
        local = Path(local_path)
        remote = Path(remote_path)
        if local.is_dir():
            self.make_dir(box_id, remote.as_posix(), recursive=True)
            for child in sorted(local.iterdir(), key=lambda item: item.name):
                self.upload_path(box_id, child, remote / child.name)
            return
        self.write_file(box_id, remote.as_posix(), local.read_bytes(), create_parents=True)

    def download_path(self, box_id: UUID | str, remote_path: str, local_path: str | Path) -> None:
        target = Path(local_path)
        try:
            entries = self.list_files(box_id, remote_path)
        except SagensError as error:
            if not should_fallback_to_file_download(str(error)):
                raise
            file = self.read_file(box_id, remote_path, 16 * 1024 * 1024)
            destination = resolve_download_file_path(remote_path, target)
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_bytes(file.data)
            return
        if _is_single_file_listing(remote_path, entries):
            file = self.read_file(box_id, remote_path, 16 * 1024 * 1024)
            destination = resolve_download_file_path(remote_path, target)
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_bytes(file.data)
            return
        target.mkdir(parents=True, exist_ok=True)
        for entry in entries:
            destination = target / entry.path
            if entry.kind.value == "directory":
                destination.mkdir(parents=True, exist_ok=True)
                continue
            if entry.kind.value == "symlink":
                raise SagensError(f"downloading symlinks is not supported yet: {entry.path}")
            file = self.read_file(box_id, f"/workspace/{entry.path}", 16 * 1024 * 1024)
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_bytes(file.data)

    def _collect_exec(self, request: dict[str, Any]) -> CompletedExecution:
        request_id = self._transport.next_request_id()
        request["request_id"] = request_id
        _, events = self._transport.open_exec_stream(request)
        stdout = bytearray()
        stderr = bytearray()
        output = bytearray()
        exit_status = None
        while True:
            event = events.get()
            if isinstance(event, Exception):
                raise event
            if event["type"] == "exec_output":
                payload = base64.b64decode(event["data"])
                output.extend(payload)
                if event["stream"] == "stdout":
                    stdout.extend(payload)
                else:
                    stderr.extend(payload)
                continue
            exit_status = exec_exit_from_raw(event["status"])
            break
        return CompletedExecution(
            exit_status=exit_status or exec_exit_from_raw("killed"),
            exit_code=exit_status.code if exit_status and exit_status.kind == "exit_code" else (0 if exit_status and exit_status.success else None),
            output=bytes(output),
            stdout=bytes(stdout),
            stderr=bytes(stderr),
        )

    def _open_shell(self, box_id: UUID | str, target: str) -> BoxShell:
        response = self._request(
            {"type": "open_shell", "box_id": str(box_id), "target": target},
            "shell_opened",
        )
        shell_id = response["shell_id"]
        return BoxShell(self._transport, shell_id, self._transport.register_shell(shell_id))

    def _request(self, request: dict[str, Any], expected_type: str) -> dict[str, Any]:
        request_id = self._transport.next_request_id()
        request["request_id"] = request_id
        return self._transport.request_response(request, expected_type)


def _is_single_file_listing(remote_path: str, entries: list[FileNode]) -> bool:
    if len(entries) != 1 or entries[0].kind.value != "file":
        return False
    relative_remote = remote_path.removeprefix("/workspace/").strip("/")
    return bool(relative_remote) and entries[0].path == relative_remote
