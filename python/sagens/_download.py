from __future__ import annotations

from pathlib import Path


def should_fallback_to_file_download(message: str) -> bool:
    return "reading workspace directory" in message and (
        "Not a directory" in message or "os error 20" in message
    )


def resolve_download_file_path(remote_path: str, local_path: Path) -> Path:
    if local_path.is_dir():
        return local_path / remote_file_name(remote_path)
    return local_path


def remote_file_name(remote_path: str) -> str:
    trimmed = remote_path.rstrip("/")
    if not trimmed:
        return "downloaded-file"
    return Path(trimmed).name or "downloaded-file"
