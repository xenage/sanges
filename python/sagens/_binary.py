from __future__ import annotations

import os
from pathlib import Path


def resolve_host_binary() -> str:
    explicit = os.environ.get("SAGENS_PYTHON_HOST_BINARY")
    if explicit:
        return explicit
    packaged = Path(__file__).resolve().parent / "_bin" / "sagens"
    if packaged.is_file():
        return str(packaged)
    repo_binary = Path(__file__).resolve().parents[2] / "target" / "debug" / "sagens"
    if repo_binary.is_file():
        return str(repo_binary)
    return str(packaged)
