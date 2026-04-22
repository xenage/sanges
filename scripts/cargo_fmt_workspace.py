#!/usr/bin/env python3
"""Run cargo fmt against first-party workspace packages only."""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent


def workspace_packages() -> list[str]:
    metadata_json = subprocess.check_output(
        ["cargo", "metadata", "--format-version", "1", "--no-deps"],
        cwd=ROOT,
        text=True,
    )
    metadata = json.loads(metadata_json)
    packages_by_id = {package["id"]: package["name"] for package in metadata["packages"]}
    return [packages_by_id[package_id] for package_id in metadata["workspace_members"]]


def main() -> int:
    command = ["cargo", "fmt", *sys.argv[1:]]
    for package in workspace_packages():
        command.extend(["-p", package])
    return subprocess.run(command, cwd=ROOT).returncode


if __name__ == "__main__":
    raise SystemExit(main())
