from __future__ import annotations

import shutil
import sys
from pathlib import Path


def _sync_local_bindings_from_installed_package() -> None:
    package_root = Path(__file__).resolve().parents[1]
    installed_root = _installed_package_root(package_root)
    if installed_root is None:
        return
    bindings_name = _installed_bindings_name(installed_root)
    _sync_file(
        installed_root / "sagens" / bindings_name if bindings_name else None,
        package_root / "sagens",
    )
    _sync_file(
        installed_root / "sagens" / "_bin" / "sagens",
        package_root / "sagens" / "_bin",
    )


def _installed_package_root(package_root: Path) -> Path | None:
    for entry in sys.path:
        if not entry or _same_path(entry, package_root):
            continue
        candidate_root = Path(entry)
        if not candidate_root.is_dir():
            continue
        package_dir = candidate_root / "sagens"
        if not package_dir.is_dir():
            continue
        if _installed_bindings_name(candidate_root) is not None:
            return candidate_root
    return None


def _installed_bindings_name(package_root: Path) -> str | None:
    for candidate in (package_root / "sagens").glob("_rust*.*"):
        if candidate.is_file():
            return candidate.name
    return None


def _sync_file(source: Path | str | None, local_dir: Path) -> None:
    if not source:
        return
    source_path = Path(source)
    if not source_path.is_file():
        return
    local = local_dir / source_path.name
    if local.exists() and local.read_bytes() == source_path.read_bytes():
        return
    local.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source_path, local)


def _same_path(entry: str, path: Path) -> bool:
    if not entry:
        return False
    try:
        return Path(entry).resolve() == path
    except OSError:
        return False


_sync_local_bindings_from_installed_package()
