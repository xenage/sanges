from __future__ import annotations

import tempfile
from pathlib import Path

import pytest

from sagens._shell import ShellExitEvent, ShellOutputEvent

from .support import create_e2e_box, e2e_daemon, e2e_enabled, real_daemon, wheelhouse


@pytest.mark.skipif(not e2e_enabled(), reason="set SAGENS_RUN_E2E=1 to run full python e2e")
def test_packaged_daemon_starts_and_creates_box_records() -> None:
    with real_daemon() as daemon:
        box = daemon.create_box()
        assert box.record.status.value == "created"
        records = daemon.list_boxes()
        assert any(record.box_id == box.box_id for record in records)


@pytest.mark.skipif(not e2e_enabled(), reason="set SAGENS_RUN_E2E=1 to run full python e2e")
def test_box_lifecycle_preserves_workspace_across_restart() -> None:
    with wheelhouse() as wheelhouse_path:
        with e2e_daemon() as daemon:
            box = create_e2e_box(daemon)
            box.start()

            python = box.exec_python(
                [
                    "-c",
                    "from pathlib import Path; Path('box.txt').write_text('persisted'); print('python-e2e')",
                ]
            )
            assert python.exit_status.success

            box.fs.upload(wheelhouse_path, ".wheelhouse")
            pip = box.exec_bash(
                "python3 -m pip install --no-index --find-links .wheelhouse --target .sandbox-pkgs sagens-e2e-pkg"
            )
            assert pip.exit_status.success

            box.stop()
            box.start()

            verify = box.exec_python(
                [
                    "-c",
                    "import sys; sys.path.insert(0, '.sandbox-pkgs'); import sagens_e2e_pkg; print(open('box.txt').read(), sagens_e2e_pkg.NAME)",
                ]
            )
            assert verify.exit_status.success
            stdout = verify.stdout.decode()
            assert "persisted" in stdout
            assert "wheel-ok" in stdout


@pytest.mark.skipif(not e2e_enabled(), reason="set SAGENS_RUN_E2E=1 to run full python e2e")
def test_box_shell_and_fs_roundtrip() -> None:
    with tempfile.TemporaryDirectory(prefix="sagens-python-fs-") as temp_dir:
        downloaded = Path(temp_dir) / "roundtrip.txt"
        with e2e_daemon() as daemon:
            box = create_e2e_box(daemon)
            box.start()

            box.fs.write("/workspace/roundtrip.txt", b"fs-e2e")
            box.fs.download("/workspace/roundtrip.txt", downloaded)
            assert downloaded.read_bytes() == b"fs-e2e"
            assert any(change.path == "roundtrip.txt" for change in box.fs.diff())

            shell = box.open_bash()
            shell.send_input("printf 'shell-e2e\\n'\nexit\n")
            outputs: list[bytes] = []
            exit_code = None
            for event in shell.iter_events():
                if isinstance(event, ShellOutputEvent):
                    outputs.append(event.data)
                if isinstance(event, ShellExitEvent):
                    exit_code = event.code
                    break

            assert exit_code == 0
            assert "shell-e2e" in b"".join(outputs).decode(errors="ignore")
