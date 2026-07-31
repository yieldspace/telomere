#!/usr/bin/env python3
"""Measure telomere-cli cold start and peak RSS.

Run from the repository root after `cargo build --release`:

    python3 docs/benchmarks/measure-cold-start.py

Cold start is whole-process wall time (exec through exit), reported as the
median of `RUNS` runs after `WARMUP` warm-up runs. Peak RSS is read from
`/usr/bin/time -l` (macOS) or `/usr/bin/time -v` (Linux) and reported as the
median of `RSS_RUNS` runs.

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
import statistics
import subprocess
import sys

RUNS = 30
WARMUP = 3
RSS_RUNS = 5

CLI = os.path.join("target", "release", "telomere-cli")
JIT_CLI = os.environ.get("TELOMERE_JIT_BIN")

MACOS_RSS = re.compile(r"^\s*(\d+)\s+maximum resident set size", re.M)
LINUX_RSS = re.compile(r"Maximum resident set size \(kbytes\):\s*(\d+)")


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


def measure(name: str, cmd: "list[str]") -> dict:
    for _ in range(WARMUP):
        subprocess.run(cmd, capture_output=True)

    times = []
    for _ in range(RUNS):
        start = time_ns()
        subprocess.run(cmd, capture_output=True)
        times.append((time_ns() - start) / 1_000_000.0)

    samples = []
    for _ in range(RSS_RUNS):
        completed = subprocess.run(
            time_argv() + cmd, capture_output=True, text=True
        )
        value = rss_bytes(completed.stderr)
        if value is not None:
            samples.append(value)

    result = {
        "case": name,
        "command": " ".join(cmd),
        "runs": RUNS,
        "wall_ms_median": round(statistics.median(times), 2),
        "wall_ms_min": round(min(times), 2),
        "wall_ms_max": round(max(times), 2),
    }
    if samples:
        median_rss = statistics.median(samples)
        result["peak_rss_bytes_median"] = int(median_rss)
        result["peak_rss_kib_median"] = round(median_rss / 1024, 1)
        result["peak_rss_runs"] = len(samples)
    return result


def time_ns() -> int:
    import time

    return time.perf_counter_ns()


def main() -> int:
    if not os.path.exists(CLI):
        print(f"missing {CLI}; run `cargo build --release` first", file=sys.stderr)
        return 1

    cases = [
        ("add.wasm core call, interpreter", [CLI, "examples/add.wasm", "main", "1", "2"]),
        ("wasi-preview1-hello.wasm preview1 command", [CLI, "examples/wasi-preview1-hello.wasm"]),
        (
            "wasi-component-args.wasm WASI 0.2 component",
            [CLI, "component", "examples/wasi-component-args.wasm", "--", "one"],
        ),
    ]
    if JIT_CLI:
        cases.insert(
            1,
            (
                "add.wasm core call, --jit",
                [JIT_CLI, "--jit", "examples/add.wasm", "main", "1", "2"],
            ),
        )

    json.dump(
        {
            "platform": platform.platform(),
            "machine": platform.machine(),
            "results": [measure(name, cmd) for name, cmd in cases],
        },
        sys.stdout,
        indent=2,
    )
    print()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
