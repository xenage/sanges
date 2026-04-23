from __future__ import annotations

from pathlib import Path
from uuid import UUID

from ._client import BoxApiClient
from ._models import BoxRecord, CheckpointRestoreMode, WorkspaceCheckpointRecord
from ._shell import BoxShell


class BoxFs:
    def __init__(self, client: BoxApiClient, box_id: UUID) -> None:
        self._client = client
        self._box_id = box_id

    def list(self, path: str = "/workspace") -> list:
        return self._client.list_files(self._box_id, path)

    def read(self, path: str, limit: int = 16 * 1024 * 1024):
        return self._client.read_file(self._box_id, path, limit)

    def write(self, path: str, data: bytes, create_parents: bool = True) -> None:
        self._client.write_file(self._box_id, path, data, create_parents=create_parents)

    def mkdir(self, path: str, recursive: bool = True) -> None:
        self._client.make_dir(self._box_id, path, recursive=recursive)

    def remove(self, path: str, recursive: bool = False) -> None:
        self._client.remove_path(self._box_id, path, recursive=recursive)

    def diff(self) -> list:
        return self._client.list_changes(self._box_id)

    def upload(self, local_path: str | Path, remote_path: str | Path) -> None:
        self._client.upload_path(self._box_id, local_path, remote_path)

    def download(self, remote_path: str, local_path: str | Path) -> None:
        self._client.download_path(self._box_id, remote_path, local_path)


class BoxCheckpoint:
    def __init__(self, client: BoxApiClient, box_id: UUID) -> None:
        self._client = client
        self._box_id = box_id

    def create(
        self,
        name: str | None = None,
        metadata: dict[str, str] | None = None,
    ) -> WorkspaceCheckpointRecord:
        return self._client.checkpoint_create(self._box_id, name, metadata)

    def list(self) -> list[WorkspaceCheckpointRecord]:
        return self._client.checkpoint_list(self._box_id)

    def restore(
        self,
        checkpoint_id: str,
        mode: CheckpointRestoreMode | str = CheckpointRestoreMode.ROLLBACK,
    ) -> WorkspaceCheckpointRecord:
        return self._client.checkpoint_restore(self._box_id, checkpoint_id, mode)

    def fork(self, checkpoint_id: str, new_box_name: str | None = None) -> BoxRecord:
        return self._client.checkpoint_fork(self._box_id, checkpoint_id, new_box_name)

    def delete(self, checkpoint_id: str) -> None:
        self._client.checkpoint_delete(self._box_id, checkpoint_id)


class Box:
    def __init__(self, client: BoxApiClient, record: BoxRecord) -> None:
        self._client = client
        self.record = record
        self.fs = BoxFs(client, record.box_id)
        self.checkpoint = BoxCheckpoint(client, record.box_id)

    @property
    def box_id(self) -> UUID:
        return self.record.box_id

    def refresh(self) -> BoxRecord:
        self.record = self._client.get_box(self.box_id)
        return self.record

    def start(self) -> BoxRecord:
        self.record = self._client.start_box(self.box_id)
        return self.record

    def stop(self) -> BoxRecord:
        self.record = self._client.stop_box(self.box_id)
        return self.record

    def remove(self) -> None:
        self._client.remove_box(self.box_id)

    def set(self, setting: str, value: bool | int) -> BoxRecord:
        self.record = self._client.set_box_setting(self.box_id, setting, value)
        return self.record

    def exec_bash(self, command: str):
        return self._client.exec_bash(self.box_id, command)

    def exec_python(self, args: list[str]):
        return self._client.exec_python(self.box_id, args)

    def exec_bash_with_timeout(self, command: str, timeout_ms: int, kill_grace_ms: int):
        return self._client.exec_bash_with_timeout(
            self.box_id,
            command,
            timeout_ms,
            kill_grace_ms,
        )

    def open_bash(self) -> BoxShell:
        return self._client.open_bash(self.box_id)

    def open_python(self) -> BoxShell:
        return self._client.open_python(self.box_id)
