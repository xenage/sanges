from __future__ import annotations

import json
import os
import socket
import tempfile
import zipfile
from base64 import urlsafe_b64encode
from contextlib import contextmanager
from hashlib import sha256
from pathlib import Path
from typing import Iterator

from sagens import Daemon
from sagens._binary import resolve_host_binary
from sagens._decode import user_config_from_dict
from sagens import _rust


@contextmanager
def smoke_server(mode: str = "compat") -> Iterator[Daemon]:
    handle = _rust.start_test_server(mode)
    daemon = Daemon.connect(**_config_kwargs(handle.user_config_json))
    try:
        yield daemon
    finally:
        daemon.close()
        handle.close()


@contextmanager
def real_daemon() -> Iterator[Daemon]:
    with tempfile.TemporaryDirectory(prefix="sagens-python-e2e-") as temp_dir:
        state_dir = Path(temp_dir)
        config_path = state_dir / "config.json"
        endpoint = f"ws://127.0.0.1:{free_port()}"
        host_binary = resolve_host_binary()
        with env_override(
            SAGENS_LIBKRUN_RUNNER="self-subprocess",
            SAGENS_LIBKRUN_RUNNER_EXE=host_binary,
        ):
            daemon = Daemon.start(
                host_binary=host_binary,
                state_dir=state_dir,
                user_config_path=config_path,
                endpoint=endpoint,
            )
        try:
            yield daemon
        finally:
            daemon.quit()
            daemon.close()


@contextmanager
def e2e_daemon() -> Iterator[Daemon]:
    if real_box_runtime_supported():
        with real_daemon() as daemon:
            yield daemon
        return
    with smoke_server() as daemon:
        yield daemon


def e2e_enabled() -> bool:
    return os.environ.get("SAGENS_RUN_E2E", "").lower() in {"1", "true", "yes"}


def real_box_runtime_supported() -> bool:
    return True


@contextmanager
def wheelhouse() -> Iterator[Path]:
    external = os.environ.get("SAGENS_WHEELHOUSE")
    if external:
        yield Path(external)
        return
    with tempfile.TemporaryDirectory(prefix="sagens-python-wheelhouse-") as temp_dir:
        output_dir = Path(temp_dir)
        build_test_wheel(output_dir)
        yield output_dir


def free_port() -> int:
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return int(sock.getsockname()[1])


@contextmanager
def env_override(**values: str) -> Iterator[None]:
    previous = {key: os.environ.get(key) for key in values}
    for key, value in values.items():
        os.environ[key] = value
    try:
        yield
    finally:
        for key, value in previous.items():
            if value is None:
                os.environ.pop(key, None)
            else:
                os.environ[key] = value


def _config_kwargs(payload: str) -> dict[str, object]:
    config = user_config_from_dict(json.loads(payload))
    return {
        "endpoint": config.endpoint,
        "admin_uuid": config.admin_uuid,
        "admin_token": config.admin_token,
    }


def build_test_wheel(output_dir: Path) -> Path:
    distribution = "sagens_e2e_pkg"
    version = "0.1.0"
    dist_info = f"{distribution}-{version}.dist-info"
    wheel_name = f"{distribution}-{version}-py3-none-any.whl"
    files = {
        f"{distribution}.py": b"NAME = 'wheel-ok'\n",
        f"{dist_info}/METADATA": (
            "Metadata-Version: 2.1\n"
            "Name: sagens-e2e-pkg\n"
            f"Version: {version}\n"
            "Summary: sagens python e2e test wheel\n"
        ).encode(),
        f"{dist_info}/WHEEL": (
            "Wheel-Version: 1.0\n"
            "Generator: sagens-tests\n"
            "Root-Is-Purelib: true\n"
            "Tag: py3-none-any\n"
        ).encode(),
        f"{dist_info}/top_level.txt": f"{distribution}\n".encode(),
    }
    wheel_path = output_dir / wheel_name
    with zipfile.ZipFile(wheel_path, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        records = []
        for name, data in files.items():
            archive.writestr(name, data)
            records.append(f"{name},{record_hash(data)},{len(data)}")
        records.append(f"{dist_info}/RECORD,,")
        archive.writestr(f"{dist_info}/RECORD", "\n".join(records) + "\n")
    return wheel_path


def record_hash(data: bytes) -> str:
    digest = urlsafe_b64encode(sha256(data).digest()).decode().rstrip("=")
    return f"sha256={digest}"
