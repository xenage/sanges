from __future__ import annotations

import os
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent
PYTHON_MANIFEST = ROOT / "python" / "Cargo.toml"


def build_wheel(
    wheel_directory: str,
    config_settings: dict[str, object] | None = None,
    metadata_directory: str | None = None,
) -> str:
    _stage_host_binary(config_settings)
    target_dir = Path(wheel_directory)
    before = {path.name for path in target_dir.glob("*.whl")}
    _run(
        _maturin_command()
        + [
            "build",
            "--manifest-path",
            str(PYTHON_MANIFEST),
            "--out",
            wheel_directory,
            "--interpreter",
            sys.executable,
            "--release",
        ]
    )
    return _new_artifact(target_dir, before, "*.whl")


def build_sdist(
    sdist_directory: str,
    config_settings: dict[str, object] | None = None,
) -> str:
    target_dir = Path(sdist_directory)
    before = {path.name for path in target_dir.glob("*.tar.gz")}
    _run(
        _maturin_command()
        + [
            "sdist",
            "--manifest-path",
            str(PYTHON_MANIFEST),
            "--out",
            sdist_directory,
        ]
    )
    return _new_artifact(target_dir, before, "*.tar.gz")


def get_requires_for_build_wheel(
    config_settings: dict[str, object] | None = None,
) -> list[str]:
    return []


def get_requires_for_build_sdist(
    config_settings: dict[str, object] | None = None,
) -> list[str]:
    return []


def _stage_host_binary(config_settings: dict[str, object] | None) -> None:
    command = [
        "cargo",
        "run",
        "--bin",
        "xtask",
        "--",
        "dev",
        "--release",
        "--skip-guest-refresh",
        "--python-package-root",
        "python",
    ]
    if _debug_build_requested(config_settings):
        command.remove("--release")
    _run(command)


def _debug_build_requested(config_settings: dict[str, object] | None) -> bool:
    if not config_settings:
        return False
    for key in ("debug", "editable_debug"):
        value = config_settings.get(key)
        if isinstance(value, str) and value.lower() in {"1", "true", "yes"}:
            return True
    return False


def _new_artifact(target_dir: Path, before: set[str], pattern: str) -> str:
    after = {path.name for path in target_dir.glob(pattern)}
    created = sorted(after - before)
    if not created:
        raise RuntimeError(f"no new artifact matching {pattern} was created")
    return created[-1]


def _maturin_command() -> list[str]:
    script = shutil.which("maturin")
    if script:
        return [script]
    return [sys.executable, "-m", "maturin"]


def _run(command: list[str]) -> None:
    subprocess.run(command, cwd=ROOT, check=True, env=os.environ.copy())
