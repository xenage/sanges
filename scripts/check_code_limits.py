#!/usr/bin/env python3

from __future__ import annotations

from pathlib import Path
import sys

MAX_LINES = 400
CHECK_SUFFIXES = {".py", ".rs"}
EXCLUDED_PARTS = {
    ".box-artifacts",
    ".build-local-target",
    ".e2e-artifacts",
    ".e2e-wheelhouse",
    ".git",
    ".sagens-state",
    ".tmp",
    ".venv",
    "artifacts",
    "dist",
    "target",
}
EXCLUDED_PREFIXES = {
    ("third_party", "runtime"),
    ("third_party", "upstream"),
}


def is_excluded(path: Path) -> bool:
    parts = path.parts
    if EXCLUDED_PARTS.intersection(parts):
        return True
    return any(parts[: len(prefix)] == prefix for prefix in EXCLUDED_PREFIXES)


def count_lines(path: Path) -> int:
    with path.open("r", encoding="utf-8") as handle:
        return sum(1 for _ in handle)


def main() -> int:
    root = Path(__file__).resolve().parents[1]
    offenders: list[tuple[Path, int]] = []

    for path in root.rglob("*"):
        if not path.is_file() or path.suffix not in CHECK_SUFFIXES:
            continue
        relative_path = path.relative_to(root)
        if is_excluded(relative_path):
            continue
        line_count = count_lines(path)
        if line_count > MAX_LINES:
            offenders.append((relative_path, line_count))

    if not offenders:
        print(f"CODE.md line-count gate passed ({MAX_LINES} lines max).")
        return 0

    print(f"CODE.md line-count gate failed ({MAX_LINES} lines max):")
    for relative_path, line_count in sorted(offenders):
        print(f"  {line_count:4} {relative_path}")
    return 1


if __name__ == "__main__":
    sys.exit(main())
