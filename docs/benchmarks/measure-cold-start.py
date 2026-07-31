#!/usr/bin/env python3
"""Measure telomere-cli cold start and peak RSS.

Run from the repository root after `cargo build --release`:

    python3 docs/benchmarks/measure-cold-start.py

Cold start is whole-process wall time (exec through exit), reported as the
median of `RUNS` runs after `WARMUP` warm-up runs. Peak RSS is read from
`/usr/bin/time -l` (macOS) or `/usr/bin/time -v` (Linux) and reported as the
median of `RSS_RUNS` runs.

Every invocation is checked. A run that exits non-zero, or whose `/usr/bin/time`
output cannot be parsed, aborts the whole measurement: the error path through
the CLI is faster than the path being measured, so recording it would produce a
plausible-looking number that means nothing.

To include the `--jit` row, build a JIT binary and point `TELOMERE_JIT_BIN` at
it:

    cargo build --release --features jit
    cp target/release/telomere-cli /tmp/telomere-cli-jit
    TELOMERE_JIT_BIN=/tmp/telomere-cli-jit python3 docs/benchmarks/measure-cold-start.py

Output is JSON on stdout.
"""

import json
import os
import platform
import re
import shlex
import statistics
import subprocess
import sys
import time

RUNS = 30
WARMUP = 3
RSS_RUNS = 5

CLI = os.path.join("target", "release", "telomere-cli")
JIT_CLI = os.environ.get("TELOMERE_JIT_BIN")

MACOS_RSS = re.compile(r"^\s*(\d+)\s+maximum resident set size", re.M)
LINUX_RSS = re.compile(r"Maximum resident set size \(kbytes\):\s*(\d+)")


class MeasurementError(RuntimeError):
    """A measurement could not be taken, so no number should be reported."""


def rss_bytes(stderr: str) -> "int | None":
    match = MACOS_RSS.search(stderr)
    if match:
        return int(match.group(1))
    match = LINUX_RSS.search(stderr)
    if match:
        return int(match.group(1)) * 1024
    return None


def time_argv() -> "list[str]":
    return ["/usr/bin/time", "-l" if platform.system() == "Darwin" else "-v"]


def run(cmd: "list[str]") -> str:
    """Run `cmd` to completion and return its stderr.

    Guest stdout is discarded; stderr is kept so that a failure can be
    reported, and so that the `/usr/bin/time` report can be parsed. A non-zero
    exit raises `subprocess.CalledProcessError`.
    """
    completed = subprocess.run(
        cmd,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE,
        text=True,
        check=True,
    )
    return completed.stderr


def measure(name: str, cmd: "list[str]") -> dict:
    for _ in range(WARMUP):
        run(cmd)

    times = []
    for _ in range(RUNS):
        start = time.perf_counter_ns()
        run(cmd)
        times.append((time.perf_counter_ns() - start) / 1_000_000.0)

    timed_cmd = time_argv() + cmd
    samples = []
    for _ in range(RSS_RUNS):
        stderr = run(timed_cmd)
        value = rss_bytes(stderr)
        if value is None:
            raise MeasurementError(
                f"could not find peak RSS in the output of "
                f"`{shlex.join(timed_cmd)}`:\n{stderr}"
            )
        samples.append(value)

    median_rss = statistics.median(samples)
    return {
        "case": name,
        "command": shlex.join(cmd),
        "runs": RUNS,
        "wall_ms_median": round(statistics.median(times), 2),
        "wall_ms_min": round(min(times), 2),
        "wall_ms_max": round(max(times), 2),
        "peak_rss_bytes_median": int(median_rss),
        "peak_rss_kib_median": round(median_rss / 1024, 1),
        "peak_rss_runs": len(samples),
    }


def cases() -> "list[tuple[str, list[str]]]":
    selected = [
        (
            "add.wasm core call, interpreter",
            [CLI, "examples/add.wasm", "main", "1", "2"],
        ),
        (
            "wasi-preview1-hello.wasm preview1 command",
            [CLI, "examples/wasi-preview1-hello.wasm"],
        ),
        (
            "wasi-component-args.wasm WASI 0.2 component",
            [CLI, "component", "examples/wasi-component-args.wasm", "--", "one"],
        ),
    ]
    if JIT_CLI:
        selected.insert(
            1,
            (
                "add.wasm core call, --jit",
                [JIT_CLI, "--jit", "examples/add.wasm", "main", "1", "2"],
            ),
        )
    return selected


def main() -> int:
    if not os.path.exists(CLI):
        print(f"missing {CLI}; run `cargo build --release` first", file=sys.stderr)
        return 1
    if JIT_CLI and not os.path.exists(JIT_CLI):
        print(
            f"TELOMERE_JIT_BIN points at {JIT_CLI}, which does not exist",
            file=sys.stderr,
        )
        return 1

    try:
        results = [measure(name, cmd) for name, cmd in cases()]
    except subprocess.CalledProcessError as error:
        print(
            f"`{shlex.join(error.cmd)}` exited with status {error.returncode}",
            file=sys.stderr,
        )
        if error.stderr:
            print(error.stderr.rstrip("\n"), file=sys.stderr)
        print(
            "refusing to report timings taken from a failing command",
            file=sys.stderr,
        )
        return 1
    except MeasurementError as error:
        print(error, file=sys.stderr)
        return 1

    json.dump(
        {
            "platform": platform.platform(),
            "machine": platform.machine(),
            "results": results,
        },
        sys.stdout,
        indent=2,
    )
    print()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
