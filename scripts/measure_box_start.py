from __future__ import annotations

import argparse
import json
import statistics
import sys
import time
from contextlib import nullcontext
from dataclasses import asdict, dataclass
from datetime import UTC, datetime
from pathlib import Path
from tempfile import TemporaryDirectory
from typing import Any

REPO_ROOT = Path(__file__).resolve().parents[1]
PYTHON_ROOT = REPO_ROOT / "python"
if str(PYTHON_ROOT) not in sys.path:
    sys.path.insert(0, str(PYTHON_ROOT))

from sagens import Daemon
from sagens._binary import resolve_host_binary

HELLO_PYTHON_ARGS = ["-c", "print('hello from python')"]


@dataclass(frozen=True)
class IterationResult:
    iteration: int
    box_id: str
    start_s: float
    first_python_s: float
    total_s: float
    stdout: str


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Measure BOX start latency up to the first successful hello-from-python exec.",
    )
    parser.add_argument(
        "--iterations",
        type=int,
        default=5,
        help="Number of fresh BOX iterations to measure (default: 5).",
    )
    parser.add_argument(
        "--host-binary",
        type=Path,
        help="Path to the sagens host binary. Defaults to Python package resolution.",
    )
    parser.add_argument(
        "--state-dir",
        type=Path,
        help="Reuse an explicit daemon state dir instead of a temporary directory.",
    )
    parser.add_argument(
        "--memory-mb",
        type=int,
        help="Override BOX memory for the benchmark. Defaults to the daemon BOX policy.",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="Emit the full report as JSON.",
    )
    args = parser.parse_args()
    if args.iterations <= 0:
        parser.error("--iterations must be at least 1")
    return args


def log(message: str) -> None:
    stamp = datetime.now(UTC).strftime("%H:%M:%S")
    print(f"[{stamp}] {message}", flush=True)


def benchmark_memory_mb(override: int | None) -> int | None:
    return override


def ensure_box_memory(box: Any, memory_mb: int | None) -> int | None:
    if memory_mb is None:
        return None
    settings = box.record.settings
    if settings is None:
        raise RuntimeError("BOX settings are missing from the daemon response")
    if settings.memory_mb.max < memory_mb:
        raise RuntimeError(
            f"benchmark requires memory_mb={memory_mb}, but host cap is "
            f"{settings.memory_mb.max}"
        )
    if settings.memory_mb.current < memory_mb:
        box.set("memory_mb", memory_mb)
    return memory_mb


def summarize(values: list[float]) -> dict[str, float]:
    return {
        "mean_s": round(statistics.fmean(values), 3),
        "median_s": round(statistics.median(values), 3),
        "min_s": round(min(values), 3),
        "max_s": round(max(values), 3),
    }


def build_report(
    host_binary: Path,
    endpoint: str,
    memory_mb: int | None,
    iterations: list[IterationResult],
) -> dict[str, Any]:
    start_values = [item.start_s for item in iterations]
    python_values = [item.first_python_s for item in iterations]
    total_values = [item.total_s for item in iterations]
    return {
        "measured_at_utc": datetime.now(UTC).isoformat(),
        "host": {
            "platform": platform.platform(),
            "machine": platform.machine(),
            "python": platform.python_version(),
        },
        "host_binary": str(host_binary),
        "endpoint": endpoint,
        "memory_mb": memory_mb,
        "iterations": [asdict(item) for item in iterations],
        "summary": {
            "start_s": summarize(start_values),
            "first_python_s": summarize(python_values),
            "time_to_first_python_s": summarize(total_values),
        },
    }


def cleanup_box(box: Any) -> None:
    try:
        box.refresh()
    except Exception:
        pass
    try:
        if getattr(box.record.status, "value", None) == "running":
            box.stop()
    except Exception as error:
        log(f"cleanup warning: stop failed for box {box.box_id}: {error}")
    try:
        box.remove()
    except Exception as error:
        log(f"cleanup warning: remove failed for box {box.box_id}: {error}")


def run_benchmark(args: argparse.Namespace) -> dict[str, Any]:
    host_binary = (args.host_binary or Path(resolve_host_binary())).resolve()
    if not host_binary.is_file():
        raise FileNotFoundError(f"host binary not found: {host_binary}")
    memory_mb = benchmark_memory_mb(args.memory_mb)

    state_context = (
        nullcontext(args.state_dir.resolve())
        if args.state_dir is not None
        else TemporaryDirectory(prefix="sagens-box-start-bench-")
    )
    with state_context as raw_state_dir:
        state_dir = Path(raw_state_dir)
        log(f"using host binary: {host_binary}")
        log(f"using state dir: {state_dir}")
        if memory_mb is not None:
            log(f"forcing memory_mb={memory_mb} for this host")
        with Daemon.start(host_binary=str(host_binary), state_dir=state_dir) as daemon:
            log(f"daemon started endpoint={daemon.client.endpoint}")
            results: list[IterationResult] = []
            for iteration in range(1, args.iterations + 1):
                box = daemon.create_box()
                log(f"iteration {iteration}: created box_id={box.box_id}")
                ensure_box_memory(box, memory_mb)
                try:
                    start_begin = time.perf_counter()
                    box.start()
                    start_end = time.perf_counter()
                    python = box.exec_python(HELLO_PYTHON_ARGS)
                    finished = time.perf_counter()
                    stdout = python.stdout.decode().strip()
                    if not python.exit_status.success:
                        raise RuntimeError(
                            f"iteration {iteration}: python exec failed with "
                            f"{python.exit_status.kind}"
                        )
                    if stdout != "hello from python":
                        raise RuntimeError(
                            f"iteration {iteration}: unexpected stdout {stdout!r}"
                        )
                    result = IterationResult(
                        iteration=iteration,
                        box_id=str(box.box_id),
                        start_s=round(start_end - start_begin, 3),
                        first_python_s=round(finished - start_end, 3),
                        total_s=round(finished - start_begin, 3),
                        stdout=stdout,
                    )
                    results.append(result)
                    log(
                        "iteration "
                        f"{iteration}: start={result.start_s:.3f}s "
                        f"first_python={result.first_python_s:.3f}s "
                        f"total={result.total_s:.3f}s"
                    )
                finally:
                    cleanup_box(box)
            return build_report(host_binary, daemon.client.endpoint, memory_mb, results)


def print_report(report: dict[str, Any], *, json_mode: bool) -> None:
    if json_mode:
        print(json.dumps(report, indent=2))
        return

    print()
    print("BOX start benchmark")
    print(f"Host: {report['host']['platform']} ({report['host']['machine']})")
    print(f"Python: {report['host']['python']}")
    print(f"Host binary: {report['host_binary']}")
    print(f"Endpoint: {report['endpoint']}")
    if report["memory_mb"] is not None:
        print(f"memory_mb: {report['memory_mb']}")
    print()
    print("Per iteration:")
    print("iter  start_s  first_python_s  total_s  box_id")
    for item in report["iterations"]:
        print(
            f"{item['iteration']:>4}  "
            f"{item['start_s']:>7.3f}  "
            f"{item['first_python_s']:>14.3f}  "
            f"{item['total_s']:>7.3f}  "
            f"{item['box_id']}"
        )
    print()
    print("Summary:")
    for key, label in (
        ("start_s", "box.start()"),
        ("first_python_s", "first exec_python()"),
        ("time_to_first_python_s", "start -> first hello from python"),
    ):
        summary = report["summary"][key]
        print(
            f"{label}: mean={summary['mean_s']:.3f}s "
            f"median={summary['median_s']:.3f}s "
            f"min={summary['min_s']:.3f}s "
            f"max={summary['max_s']:.3f}s"
        )


def main() -> int:
    args = parse_args()
    report = run_benchmark(args)
    print_report(report, json_mode=args.json)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
