from __future__ import annotations

import shutil
import sys
from pathlib import Path


def _sync_local_bindings_from_installed_package() -> None:
    package_root = Path(__file__).resolve().parents[1]
    installed = _installed_bindings_path(package_root)
    if installed is None:
        return
    local = package_root / "sagens" / installed.name
    if local.exists() and local.read_bytes() == installed.read_bytes():
        return
    local.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(installed, local)


def _installed_bindings_path(package_root: Path) -> Path | None:
    for entry in sys.path:
        if not entry or _same_path(entry, package_root):
            continue
        candidate_root = Path(entry)
        if not candidate_root.is_dir():
            continue
        package_dir = candidate_root / "sagens"
        if not package_dir.is_dir():
            continue
        for candidate in package_dir.glob("_rust*.*"):
            if candidate.is_file():
                return candidate
    return None


def _same_path(entry: str, path: Path) -> bool:
    if not entry:
        return False
    try:
        return Path(entry).resolve() == path
    except OSError:
        return False


_sync_local_bindings_from_installed_package()
