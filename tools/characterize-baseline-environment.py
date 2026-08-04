#!/usr/bin/env python3
"""Record host-load context without performing an interpreter measurement.

This tool deliberately does *not* apply the interpreter-baseline load gate.  It
exists to characterize whether that gate can be reached, and its JSON is always
marked ineligible for publication as a baseline.
"""

from __future__ import annotations

import argparse
import datetime as datetime_module
import json
import math
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
from collections.abc import Callable, Sequence


SCHEMA_VERSION = 1
PURPOSE = "environment_characterization"
ISSUE = "yieldspace/telomere#184"
GATE_BYPASS_REASON = "characterization_not_timing_measurement"
DEFAULT_DURATION_SECONDS = 300.0
DEFAULT_INTERVAL_SECONDS = 15.0
DEFAULT_TOP_COUNT = 12
DEFAULT_OUT = Path("/private/tmp/telomere-184-environment-characterization.json")


class CharacterizationError(RuntimeError):
    """A host observation could not be recorded faithfully."""


def utc_now() -> str:
    """Return an unambiguous UTC timestamp for JSON artifacts."""

    return (
        datetime_module.datetime.now(datetime_module.timezone.utc)
        .isoformat()
        .replace("+00:00", "Z")
    )


def positive_float(value: str) -> float:
    """Parse a finite positive duration or interval."""

    try:
        parsed = float(value)
    except ValueError as error:
        raise argparse.ArgumentTypeError("must be a number") from error
    if not math.isfinite(parsed) or parsed <= 0:
        raise argparse.ArgumentTypeError("must be finite and greater than zero")
    return parsed


def positive_integer(value: str) -> int:
    """Parse a positive process-count limit."""

    try:
        parsed = int(value)
    except ValueError as error:
        raise argparse.ArgumentTypeError("must be an integer") from error
    if parsed <= 0:
        raise argparse.ArgumentTypeError("must be greater than zero")
    return parsed


def parse_process_rows(stdout: str, top_count: int) -> list[dict[str, object]]:
    """Parse the portable fields requested from ``ps`` and keep CPU leaders."""

    rows: list[dict[str, object]] = []
    for line in stdout.splitlines():
        fields = line.strip().split(None, 6)
        if len(fields) != 7:
            continue
        pid, parent_pid, cpu, memory, elapsed, state, command = fields
        try:
            cpu_percent = float(cpu)
            memory_percent = float(memory)
            if not math.isfinite(cpu_percent) or not math.isfinite(memory_percent):
                continue
            row: dict[str, object] = {
                "pid": int(pid),
                "ppid": int(parent_pid),
                "cpu_percent": cpu_percent,
                "memory_percent": memory_percent,
                "elapsed": elapsed,
                "state": state,
                "command": command,
            }
        except ValueError:
            continue
        rows.append(row)
    rows.sort(key=lambda row: float(row["cpu_percent"]), reverse=True)
    return rows[:top_count]


def snapshot_top_processes(top_count: int) -> list[dict[str, object]]:
    """Capture the current CPU-leading processes with a structured command."""

    try:
        completed = subprocess.run(
            ["ps", "-A", "-o", "pid=,ppid=,%cpu=,%mem=,etime=,state=,comm="],
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
    except (OSError, subprocess.SubprocessError) as error:
        raise CharacterizationError(f"cannot capture process snapshot: {error}") from error
    return parse_process_rows(completed.stdout, top_count)


def _finite_loads(loads: Sequence[float]) -> tuple[float, float, float]:
    if len(loads) != 3 or not all(math.isfinite(value) for value in loads):
        raise CharacterizationError("os.getloadavg() did not return three finite values")
    return float(loads[0]), float(loads[1]), float(loads[2])


def summarize(values: Sequence[float]) -> dict[str, float]:
    """Return the requested extrema and endpoints for one load-average series."""

    if not values:
        raise CharacterizationError("environment characterization recorded no samples")
    return {
        "minimum": min(values),
        "maximum": max(values),
        "first": values[0],
        "last": values[-1],
    }


def _base_payload() -> dict[str, object]:
    return {
        "schema_version": SCHEMA_VERSION,
        "purpose": PURPOSE,
        "issue": ISSUE,
        "published_baseline_eligible": False,
        "timing_measurement": False,
        "load_gate_applied": False,
        "load_gate_bypassed_reason": GATE_BYPASS_REASON,
    }


def collect_characterization(
    *,
    duration_seconds: float,
    interval_seconds: float,
    top_count: int,
    now_utc: Callable[[], str] = utc_now,
    monotonic: Callable[[], float] = time.monotonic,
    sleep: Callable[[float], None] = time.sleep,
    getloadavg: Callable[[], Sequence[float]] = os.getloadavg,
    process_snapshot: Callable[[int], list[dict[str, object]]] = snapshot_top_processes,
) -> dict[str, object]:
    """Collect non-timing samples on the requested fixed cadence.

    The injectable dependencies make cadence and payload construction testable
    without waiting or observing the developer's actual machine.
    """

    if duration_seconds <= 0 or not math.isfinite(duration_seconds):
        raise CharacterizationError("duration_seconds must be finite and greater than zero")
    if interval_seconds <= 0 or not math.isfinite(interval_seconds):
        raise CharacterizationError("interval_seconds must be finite and greater than zero")
    if top_count <= 0:
        raise CharacterizationError("top_count must be greater than zero")

    started_at_utc = now_utc()
    started_monotonic = monotonic()
    samples: list[dict[str, object]] = []
    sample_index = 0

    while True:
        elapsed_seconds = monotonic() - started_monotonic
        one_minute, five_minutes, fifteen_minutes = _finite_loads(getloadavg())
        samples.append(
            {
                "sample_index": sample_index,
                "timestamp_utc": now_utc(),
                "elapsed_seconds": round(elapsed_seconds, 3),
                "load_average": {
                    "one_minute": one_minute,
                    "five_minutes": five_minutes,
                    "fifteen_minutes": fifteen_minutes,
                },
                "top_processes": process_snapshot(top_count),
            }
        )
        if elapsed_seconds >= duration_seconds:
            break

        sample_index += 1
        deadline = started_monotonic + min(
            duration_seconds, sample_index * interval_seconds
        )
        sleep(max(0.0, deadline - monotonic()))

    one_minute_series = [
        float(sample["load_average"]["one_minute"]) for sample in samples
    ]
    five_minute_series = [
        float(sample["load_average"]["five_minutes"]) for sample in samples
    ]
    fifteen_minute_series = [
        float(sample["load_average"]["fifteen_minutes"]) for sample in samples
    ]
    payload = _base_payload()
    payload.update(
        {
            "status": "observed",
            "requested_duration_seconds": duration_seconds,
            "sampling_interval_seconds": interval_seconds,
            "top_process_count": top_count,
            "sample_count": len(samples),
            "started_at_utc": started_at_utc,
            "ended_at_utc": now_utc(),
            "observed_duration_seconds": round(monotonic() - started_monotonic, 3),
            "load_average_series": {
                "one_minute": one_minute_series,
                "five_minutes": five_minute_series,
                "fifteen_minutes": fifteen_minute_series,
            },
            "load_average_summary": {
                "one_minute": summarize(one_minute_series),
                "five_minutes": summarize(five_minute_series),
                "fifteen_minutes": summarize(fifteen_minute_series),
            },
            "one_minute_load_summary": summarize(one_minute_series),
            "samples": samples,
        }
    )
    return payload


def error_payload(
    *, started_at_utc: str, ended_at_utc: str, error: Exception
) -> dict[str, object]:
    """Make a non-measurement failure inspectable when an output path exists."""

    payload = _base_payload()
    payload.update(
        {
            "status": "invalid",
            "invalid_reason": "environment_characterization_error",
            "started_at_utc": started_at_utc,
            "ended_at_utc": ended_at_utc,
            "error": {"type": type(error).__name__, "message": str(error)},
        }
    )
    return payload


def write_json(output: Path, payload: dict[str, object]) -> None:
    """Atomically publish a complete JSON artifact, never a partial sample set."""

    output.parent.mkdir(parents=True, exist_ok=True)
    encoded = json.dumps(payload, indent=2, sort_keys=True) + "\n"
    temporary_name: str | None = None
    try:
        with tempfile.NamedTemporaryFile(
            "w", encoding="utf-8", dir=output.parent, delete=False
        ) as temporary:
            temporary_name = temporary.name
            temporary.write(encoded)
        Path(temporary_name).replace(output)
    finally:
        if temporary_name is not None:
            temporary_path = Path(temporary_name)
            if temporary_path.exists():
                temporary_path.unlink()


def run_characterization(
    *,
    output: Path,
    duration_seconds: float,
    interval_seconds: float,
    top_count: int,
    now_utc: Callable[[], str] = utc_now,
    monotonic: Callable[[], float] = time.monotonic,
    sleep: Callable[[float], None] = time.sleep,
    getloadavg: Callable[[], Sequence[float]] = os.getloadavg,
    process_snapshot: Callable[[int], list[dict[str, object]]] = snapshot_top_processes,
) -> int:
    """Write either an observation or a structured error artifact."""

    started_at_utc = now_utc()
    try:
        payload = collect_characterization(
            duration_seconds=duration_seconds,
            interval_seconds=interval_seconds,
            top_count=top_count,
            now_utc=now_utc,
            monotonic=monotonic,
            sleep=sleep,
            getloadavg=getloadavg,
            process_snapshot=process_snapshot,
        )
    except Exception as error:  # Preserve diagnostics rather than silently failing.
        payload = error_payload(
            started_at_utc=started_at_utc,
            ended_at_utc=now_utc(),
            error=error,
        )
        try:
            write_json(output, payload)
        except OSError as write_error:
            print(
                json.dumps(
                    {
                        "status": "invalid",
                        "invalid_reason": "environment_characterization_error",
                        "error": str(error),
                        "artifact_write_error": str(write_error),
                    },
                    sort_keys=True,
                ),
                file=sys.stderr,
            )
        return 1

    write_json(output, payload)
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--duration",
        type=positive_float,
        default=DEFAULT_DURATION_SECONDS,
        metavar="SECONDS",
        help=f"observation duration in seconds (default: {DEFAULT_DURATION_SECONDS:g})",
    )
    parser.add_argument(
        "--interval",
        type=positive_float,
        default=DEFAULT_INTERVAL_SECONDS,
        metavar="SECONDS",
        help=f"sample cadence in seconds (default: {DEFAULT_INTERVAL_SECONDS:g})",
    )
    parser.add_argument(
        "--top-count",
        type=positive_integer,
        default=DEFAULT_TOP_COUNT,
        metavar="COUNT",
        help=f"number of CPU-leading processes per sample (default: {DEFAULT_TOP_COUNT})",
    )
    parser.add_argument(
        "--out",
        type=Path,
        default=DEFAULT_OUT,
        metavar="PATH",
        help=f"JSON artifact path (default: {DEFAULT_OUT})",
    )
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    arguments = build_parser().parse_args(argv)
    status = run_characterization(
        output=arguments.out,
        duration_seconds=arguments.duration,
        interval_seconds=arguments.interval,
        top_count=arguments.top_count,
    )
    if status == 0:
        print(arguments.out)
    return status


if __name__ == "__main__":
    raise SystemExit(main())
