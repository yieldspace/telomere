#!/usr/bin/env python3
"""Shared helpers for process timing and peak-RSS measurements."""

import platform
import re
import shlex
import statistics
import subprocess
import time
from typing import List, Optional, Sequence, Tuple


MACOS_RSS = re.compile(r"^\s*(\d+)\s+maximum resident set size", re.M)
LINUX_RSS = re.compile(r"Maximum resident set size \(kbytes\):\s*(\d+)")


class MeasurementError(RuntimeError):
    """Raised when a measurement tool did not produce usable output."""


def rss_bytes(stderr: str) -> Optional[int]:
    """Return peak RSS in bytes from macOS or GNU time output."""

    macos = MACOS_RSS.search(stderr)
    if macos:
        return int(macos.group(1))

    linux = LINUX_RSS.search(stderr)
    if linux:
        return int(linux.group(1)) * 1024

    return None


def time_argv() -> List[str]:
    """Return the platform-specific command used to collect peak RSS."""

    return ["/usr/bin/time", "-l" if platform.system() == "Darwin" else "-v"]


def run(cmd: Sequence[str], expected_stdout: Optional[str] = None) -> str:
    """Run *cmd*, returning stderr and optionally checking its exact stdout."""

    completed = subprocess.run(
        list(cmd),
        stdout=subprocess.PIPE if expected_stdout is not None else subprocess.DEVNULL,
        stderr=subprocess.PIPE,
        text=True,
        check=True,
    )
    if expected_stdout is not None and completed.stdout != expected_stdout:
        raise MeasurementError(
            f"`{shlex.join(cmd)}` printed {completed.stdout!r}; "
            f"expected {expected_stdout!r}"
        )
    return completed.stderr


def median(values: Sequence[float]) -> float:
    """Return the statistics median with one shared implementation."""

    return statistics.median(values)


def wall_ms_samples(
    cmd: Sequence[str],
    warmup: int,
    runs: int,
    expected_stdout: Optional[str] = None,
) -> List[float]:
    """Warm a process, then collect whole-process wall-clock samples in ms."""

    for _ in range(warmup):
        run(cmd, expected_stdout)

    samples = []
    for _ in range(runs):
        started = time.perf_counter_ns()
        run(cmd, expected_stdout)
        samples.append((time.perf_counter_ns() - started) / 1_000_000)

    return samples


def peak_rss_samples(
    cmd: Sequence[str],
    runs: int,
    expected_stdout: Optional[str] = None,
) -> Tuple[List[int], List[str]]:
    """Collect peak-RSS samples and return them with the timed command."""

    timed_cmd = time_argv() + list(cmd)
    samples = []
    for _ in range(runs):
        stderr = run(timed_cmd, expected_stdout)
        parsed = rss_bytes(stderr)
        if parsed is None:
            raise MeasurementError(
                "could not find peak RSS in the output of "
                f"`{shlex.join(timed_cmd)}`:\n"
                f"{stderr}"
            )
        samples.append(parsed)

    return samples, timed_cmd
