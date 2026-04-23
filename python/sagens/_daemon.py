from __future__ import annotations

import json
from pathlib import Path
from uuid import UUID

from . import _rust
from ._binary import resolve_host_binary
from ._box import Box
from ._client import BoxApiClient
from ._decode import user_config_from_dict
from ._models import BoxRecord


class Daemon:
    def __init__(
        self,
        client: BoxApiClient,
        *,
        process_handle: object | None = None,
        config_path: Path | None = None,
        host_binary: str | None = None,
    ) -> None:
        self.client = client
        self._process_handle = process_handle
        self.config_path = config_path
        self.host_binary = host_binary

    @classmethod
    def start(
        cls,
        *,
        host_binary: str | None = None,
        state_dir: str | Path | None = None,
        user_config_path: str | Path | None = None,
        endpoint: str | None = None,
    ) -> "Daemon":
        host_binary = host_binary or resolve_host_binary()
        handle = _rust.spawn_daemon_process(
            host_binary,
            str(state_dir) if state_dir else None,
            str(user_config_path) if user_config_path else None,
            endpoint,
        )
        config = user_config_from_dict(json.loads(handle.user_config_json))
        return cls(
            BoxApiClient.from_user_config(config),
            process_handle=handle,
            config_path=Path(user_config_path) if user_config_path else None,
            host_binary=host_binary,
        )

    @classmethod
    def connect(
        cls,
        endpoint: str,
        admin_uuid: UUID | str,
        admin_token: str,
    ) -> "Daemon":
        return cls(BoxApiClient.connect(endpoint, admin_uuid, admin_token))

    @classmethod
    def from_config(cls, config_path: str | Path) -> "Daemon":
        raw = json.loads(_rust.read_user_config_json(str(config_path)))
        config = user_config_from_dict(raw)
        return cls(
            BoxApiClient.from_user_config(config),
            config_path=Path(config_path),
        )

    def close(self) -> None:
        self.client.close()
        if self._process_handle is not None:
            self._process_handle.close()
            self._process_handle = None

    def quit(self) -> bool:
        if self._process_handle is not None:
            closed = bool(self._process_handle.close())
            self._process_handle = None
            return closed
        return bool(
            _rust.quit_daemon(
                None,
                str(self.config_path) if self.config_path else None,
                None,
            )
        )

    def list_boxes(self) -> list[BoxRecord]:
        return self.client.list_boxes()

    def get_box(self, box_id: UUID | str) -> Box:
        return Box(self.client, self.client.get_box(box_id))

    def create_box(self) -> Box:
        return Box(self.client, self.client.create_box())

    def issue_box_credentials(self, box_id: UUID | str):
        return self.client.issue_box_credentials(box_id)

    def connect_as_box(self, box_id: UUID | str, box_token: str | None):
        return BoxApiClient.connect_as_box(self.client.endpoint, box_id, box_token)

    def admin_add(self):
        return self.client.admin_add()

    def admin_remove_me(self) -> None:
        self.client.admin_remove_me()

    def __enter__(self) -> "Daemon":
        return self

    def __exit__(self, *_: object) -> None:
        self.close()
