from __future__ import annotations

import base64
import csv
import hashlib
import os
import re
import shutil
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path
from stat import S_IFREG, S_IMODE

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
    wheel_name = _new_artifact(target_dir, before, "*.whl")
    wheel_path = target_dir / wheel_name
    wheel_path = _finalize_wheel(wheel_path)
    return wheel_path.name


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


def _finalize_wheel(wheel_path: Path) -> Path:
    if sys.platform != "darwin" and not _wheel_build_tag():
        return wheel_path
    with tempfile.TemporaryDirectory(prefix="sagens-wheel-") as temp_dir:
        unpacked = Path(temp_dir) / "wheel"
        _extract_wheel(wheel_path, unpacked)
        if sys.platform == "darwin":
            _sign_macos_native_payload(unpacked)
        build_tag = _wheel_build_tag()
        if build_tag:
            _set_wheel_build_tag(unpacked, build_tag)
        _write_record(unpacked)
        output_path = _wheel_path_with_build_tag(wheel_path, build_tag)
        _pack_wheel(unpacked, output_path)
    if output_path != wheel_path:
        wheel_path.unlink()
    return output_path


def _wheel_build_tag() -> str | None:
    value = os.environ.get("SAGENS_WHEEL_BUILD_TAG", "").strip()
    if not value:
        return None
    if not re.fullmatch(r"[0-9][0-9A-Za-z_]*(?:\.[0-9A-Za-z_]+)*", value):
        raise RuntimeError(f"invalid SAGENS_WHEEL_BUILD_TAG: {value!r}")
    return value


def _wheel_path_with_build_tag(wheel_path: Path, build_tag: str | None) -> Path:
    if not build_tag:
        return wheel_path
    stem = wheel_path.name.removesuffix(".whl")
    parts = stem.split("-")
    if len(parts) == 5:
        parts.insert(2, build_tag)
    elif len(parts) == 6:
        parts[2] = build_tag
    else:
        raise RuntimeError(f"unexpected wheel filename layout: {wheel_path.name}")
    return wheel_path.with_name("-".join(parts) + ".whl")


def _extract_wheel(wheel_path: Path, destination: Path) -> None:
    with zipfile.ZipFile(wheel_path) as archive:
        for info in archive.infolist():
            target = destination / info.filename
            if not target.resolve().is_relative_to(destination.resolve()):
                raise RuntimeError(f"unsafe wheel member path: {info.filename}")
            if info.is_dir():
                target.mkdir(parents=True, exist_ok=True)
                continue
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(archive.read(info))
            mode = info.external_attr >> 16
            if mode:
                target.chmod(S_IMODE(mode))


def _sign_macos_native_payload(root: Path) -> None:
    candidates: list[tuple[Path, bool]] = []
    for path in sorted(root.rglob("*")):
        if not path.is_file():
            continue
        is_host = path.name == "sagens" and path.parent.name == "_bin"
        is_extension = path.suffix in {".so", ".dylib"}
        if is_host or is_extension:
            candidates.append((path, is_host))
    if not candidates:
        raise RuntimeError("macOS wheel has no native payload to sign")
    for path, is_host in candidates:
        command = [
            "cargo",
            "run",
            "--bin",
            "xtask",
            "--",
            "sign-path",
            "--path",
            str(path),
        ]
        if is_host:
            command.append("--host")
        _run(command)


def _set_wheel_build_tag(root: Path, build_tag: str) -> None:
    wheel_files = list(root.glob("*.dist-info/WHEEL"))
    if len(wheel_files) != 1:
        raise RuntimeError("expected exactly one WHEEL metadata file")
    wheel_file = wheel_files[0]
    lines = [
        line
        for line in wheel_file.read_text(encoding="utf-8").splitlines()
        if not line.startswith("Build:")
    ]
    insert_at = 1 if lines and lines[0].startswith("Wheel-Version:") else 0
    lines.insert(insert_at, f"Build: {build_tag}")
    wheel_file.write_text("\n".join(lines) + "\n", encoding="utf-8")


def _write_record(root: Path) -> None:
    record_files = list(root.glob("*.dist-info/RECORD"))
    if len(record_files) != 1:
        raise RuntimeError("expected exactly one RECORD file")
    record_file = record_files[0]
    rows: list[list[str]] = []
    for path in sorted(item for item in root.rglob("*") if item.is_file()):
        rel = path.relative_to(root).as_posix()
        if path == record_file:
            rows.append([rel, "", ""])
            continue
        data = path.read_bytes()
        digest = base64.urlsafe_b64encode(hashlib.sha256(data).digest()).decode().rstrip("=")
        rows.append([rel, f"sha256={digest}", str(len(data))])
    with record_file.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.writer(handle)
        writer.writerows(rows)


def _pack_wheel(root: Path, wheel_path: Path) -> None:
    temp_path = wheel_path.with_suffix(wheel_path.suffix + ".tmp")
    if temp_path.exists():
        temp_path.unlink()
    with zipfile.ZipFile(temp_path, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        for path in sorted(item for item in root.rglob("*") if item.is_file()):
            rel = path.relative_to(root).as_posix()
            info = zipfile.ZipInfo(rel)
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = (S_IFREG | S_IMODE(path.stat().st_mode)) << 16
            archive.writestr(info, path.read_bytes())
    temp_path.replace(wheel_path)


def _maturin_command() -> list[str]:
    script = shutil.which("maturin")
    if script:
        return [script]
    return [sys.executable, "-m", "maturin"]


def _run(command: list[str]) -> None:
    subprocess.run(command, cwd=ROOT, check=True, env=os.environ.copy())
