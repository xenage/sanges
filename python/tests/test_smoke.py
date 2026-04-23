from __future__ import annotations

import pytest

from sagens._errors import SagensError
from sagens._shell import ShellExitEvent, ShellOutputEvent

from .support import smoke_server


def test_websocket_contract_preserves_exec_and_file_flow() -> None:
    with smoke_server() as daemon:
        box = daemon.create_box()
        box.start()

        exec_result = box.exec_bash("touch tracked.txt")
        assert exec_result.exit_status.success

        changes = box.fs.diff()
        assert changes[0].path == "tracked.txt"

        box.fs.write("/workspace/tracked.txt", b"hello websocket")
        file = box.fs.read("/workspace/tracked.txt", 4096)
        assert file.data == b"hello websocket"


def test_websocket_supports_shell_checkpoint_and_box_auth() -> None:
    with smoke_server() as daemon:
        box = daemon.create_box()
        box.start()

        shell = box.open_bash()
        shell.send_input("ping\nexit\n")
        outputs: list[bytes] = []
        for event in shell.iter_events():
            if isinstance(event, ShellOutputEvent):
                outputs.append(event.data)
            if isinstance(event, ShellExitEvent):
                assert event.code == 0
        assert b"shell-ok" in b"".join(outputs)

        checkpoint = box.checkpoint.create("auth-flow")
        assert checkpoint.summary.workspace_id == str(box.box_id)
        assert checkpoint.summary.name == "auth-flow"

        bundle = daemon.issue_box_credentials(box.box_id)
        box_client = daemon.connect_as_box(box.box_id, bundle.box_token)
        with pytest.raises(SagensError):
            box_client.list_boxes()

        box_shell = box_client.open_bash(box.box_id)
        box_shell.send_input("shell-ok\nexit\n")
        for event in box_shell.iter_events():
            if isinstance(event, ShellExitEvent):
                assert event.code == 0
                break
        box_client.close()


def test_secure_mode_rejects_uuid_only_box_auth_and_accepts_token() -> None:
    with smoke_server("secure") as daemon:
        box = daemon.create_box()

        with pytest.raises(SagensError):
            daemon.connect_as_box(box.box_id, None)

        with pytest.raises(SagensError):
            daemon.connect_as_box(box.box_id, "wrong-token")

        bundle = daemon.issue_box_credentials(box.box_id)
        box_client = daemon.connect_as_box(box.box_id, bundle.box_token)
        with pytest.raises(SagensError):
            box_client.list_boxes()
        box_client.close()
