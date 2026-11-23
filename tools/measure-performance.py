#!/usr/bin/env python3
"""Measure the CI performance-gate workloads and emit one JSON record.

Issue #212 adds a performance-regression gate next to the #164 size gate.
The gate consumes the measurement methodology of
docs/core/interpreter-baseline.md rather than inventing a third convention:
the workloads are the methodology's L2 corpus (tools/baseline/artifacts.json),
the timing is its L2 whole-process wall clock through telomere-cli, and the
build/run cells are its matrix restricted to the two shipped builds.

Cells (both engines, both feature configurations):

- ``default-interp``: default build, interpreter (no ``--jit``).
- ``jit-interp``: ``jit``-feature build, interpreter (no ``--jit``). This is
  the cell that makes the JIT-feature interpreter tax visible.
- ``jit-jit``: ``jit``-feature build running with ``--jit``.

Two corpus members are deliberately not measured; see
docs/benchmarks/performance.md for the evidence: ``f64-kernel`` (the
interpreter aborts with a host stack overflow at scale on current main)
and CoreMark (the manifest's pinned digest no longer matches the artifact
served at the pinned URL, so the corpus fetch fails its own digest check).

The record's derived series are the two within-run ratios the gate
enforces (see tools/check-performance.py for why ratios, not absolute
times, are the fleet-invariant quantities): ``jit_feature_tax_ratio``
(jit-build interpreter median over default-build interpreter median) and
``jit_speed_ratio`` (JIT median over jit-build interpreter median).

Each (workload, cell) pair runs one unrecorded warm-up and then
``--repetitions`` recorded samples, sequentially and back to back, so the
two interpreter cells of a workload are measured adjacently. Every sample
self-validates through the workload's exact-stdout contract; a mismatch, a
non-zero exit, or a timeout fails the whole measurement closed. The record
keeps every raw sample, the median, and the host facts, in the spirit of
the methodology's raw-record rule: a gate number without its environment
is not evidence.

This script measures only. tools/check-performance.py compares the record
against tools/perf-baseline.json.
"""

import argparse
import json
import math
import os
import statistics
import subprocess
import sys
import time
from pathlib import Path


sys.path.insert(0, str(Path(__file__).resolve().parent))
from _measure_support import machine_facts, sha256_file  # noqa: E402


REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_MANIFEST = REPO_ROOT / "tools" / "baseline" / "artifacts.json"

# Corpus members excluded from the gate, with the reason recorded in
# docs/benchmarks/performance.md ("What is not gated").
EXCLUDED_WORKLOADS = ("coremark", "f64-kernel")

CELLS = ("default-interp", "jit-interp", "jit-jit")

SAMPLE_TIMEOUT_SECONDS = 600


class MeasureError(RuntimeError):
    """A workload, artifact, or run contract failed; fail closed."""


def cell_command(cell, binary, artifact_path, export, iterations):
    command = [str(binary)]
    if cell == "jit-jit":
        command.append("--jit")
    command.append(str(artifact_path))
    if export is not None:
        command.extend([export, str(iterations)])
    return command


def load_workloads(manifest_path):
    """Return the gated workloads from the methodology corpus manifest."""
    try:
        manifest = json.loads(Path(manifest_path).read_text(encoding="utf-8"))
    except OSError as error:
        raise MeasureError(f"could not read workload manifest {manifest_path}: {error}") from error
    except json.JSONDecodeError as error:
        raise MeasureError(f"invalid JSON in workload manifest {manifest_path}: {error}") from error
    workloads = manifest.get("workloads")
    if not isinstance(workloads, list) or not workloads:
        raise MeasureError("workload manifest lacks a workloads list")
    gated = []
    for workload in workloads:
        if not isinstance(workload, dict) or not isinstance(workload.get("name"), str):
            raise MeasureError("workload manifest contains a malformed workload entry")
        if workload["name"] in EXCLUDED_WORKLOADS:
            continue
        if workload.get("kind") != "wat":
            raise MeasureError(
                f"gated workload {workload['name']} is not a checked-in WAT workload; "
                "the gate measures only hermetic in-tree workloads"
            )
        gated.append(workload)
    if not gated:
        raise MeasureError("workload manifest produced no gated workloads")
    return gated


def prepare_workload(workload, work_dir, wasm_tools):
    """Verify a WAT workload against its manifest digest and compile it."""
    name = workload["name"]
    source_rel = workload.get("source")
    source_sha = workload.get("source_sha256")
    filename = workload.get("filename")
    if not isinstance(source_rel, str) or not isinstance(source_sha, str) or not isinstance(filename, str):
        raise MeasureError(f"workload {name} lacks source, source_sha256, or filename")
    source = REPO_ROOT / source_rel
    if not source.is_file():
        raise MeasureError(f"workload {name} source does not exist: {source}")
    actual_sha = sha256_file(source)
    if actual_sha != source_sha:
        raise MeasureError(
            f"workload {name} source {source} does not match its manifest digest "
            f"(expected {source_sha}, got {actual_sha})"
        )
    artifact = work_dir / filename
    completed = subprocess.run(
        [wasm_tools, "parse", str(source), "-o", str(artifact)],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    if completed.returncode != 0:
        raise MeasureError(
            f"wasm-tools parse failed for workload {name}: {completed.stderr.strip()}"
        )
    return artifact


def expected_stdout(workload, iterations):
    validation = workload.get("validation")
    if not isinstance(validation, dict) or validation.get("kind") != "exact_stdout":
        raise MeasureError(
            f"workload {workload['name']} has no exact_stdout validation contract; "
            "the gate only measures self-validating workloads"
        )
    template = validation.get("expected_stdout")
    if not isinstance(template, str):
        raise MeasureError(f"workload {workload['name']} lacks an expected_stdout template")
    return template.format(n=iterations)


def run_sample(command, expected):
    """Run one whole-process sample; return its record or raise MeasureError."""
    load_before = os.getloadavg()[0]
    started = time.perf_counter_ns()
    try:
        completed = subprocess.run(
            command,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            timeout=SAMPLE_TIMEOUT_SECONDS,
        )
    except subprocess.TimeoutExpired as error:
        raise MeasureError(f"sample timed out after {SAMPLE_TIMEOUT_SECONDS}s: {command}") from error
    wall_ns = time.perf_counter_ns() - started
    load_after = os.getloadavg()[0]
    if completed.returncode != 0:
        raise MeasureError(
            f"sample exited {completed.returncode}: {command}\nstderr: {completed.stderr.strip()[:500]}"
        )
    if completed.stdout != expected:
        raise MeasureError(
            f"workload printed {completed.stdout!r}; expected {expected!r} (self-validation failed)"
        )
    if not (math.isfinite(load_before) and math.isfinite(load_after)):
        raise MeasureError("load average around a sample was not finite")
    return {
        "wall_ns": wall_ns,
        "load_average_1m_before": load_before,
        "load_average_1m_after": load_after,
    }


def summarize(samples):
    walls = [sample["wall_ns"] for sample in samples]
    return {
        "samples": samples,
        "sample_count": len(walls),
        "median_ns": int(statistics.median(walls)),
        "min_ns": min(walls),
        "max_ns": max(walls),
    }


def measure_cell(cell, binary, artifact, workload, repetitions, warmups):
    iterations = workload.get("n")
    export = workload.get("export")
    if not isinstance(iterations, int) or not isinstance(export, str):
        raise MeasureError(f"workload {workload['name']} lacks an export or iteration count")
    expected = expected_stdout(workload, iterations)
    command = cell_command(cell, binary, artifact, export, iterations)
    for _ in range(warmups):
        run_sample(command, expected)
    samples = [run_sample(command, expected) for _ in range(repetitions)]
    return summarize(samples)


def measure(binaries, workloads, artifacts, repetitions, warmups):
    """Measure every (workload, cell) pair; return the measurement record."""
    record = {
        "tool": "measure-performance",
        "version": 1,
        "environment": machine_facts(),
        "cells": {},
        "derived": {},
    }
    for workload in workloads:
        name = workload["name"]
        per_cell = {}
        for cell in CELLS:
            per_cell[cell] = measure_cell(
                cell, binaries[cell], artifacts[name], workload, repetitions, warmups
            )
        record["cells"][name] = per_cell
        default_median = per_cell["default-interp"]["median_ns"]
        jit_interp_median = per_cell["jit-interp"]["median_ns"]
        jit_median = per_cell["jit-jit"]["median_ns"]
        record["derived"][name] = {
            "jit_feature_tax_ratio": {
                "value": jit_interp_median / default_median,
                "jit_interp_median_ns": jit_interp_median,
                "default_interp_median_ns": default_median,
            },
            "jit_speed_ratio": {
                "value": jit_median / jit_interp_median,
                "jit_median_ns": jit_median,
                "jit_interp_median_ns": jit_interp_median,
            },
        }
    return record


def parse_args(argv=None):
    parser = argparse.ArgumentParser(
        description="measure the performance-gate workloads and emit a JSON record"
    )
    parser.add_argument("--default-bin", required=True, help="telomere-cli built with default features")
    parser.add_argument("--jit-bin", required=True, help="telomere-cli built with --features jit")
    parser.add_argument("--wasm-tools", default="wasm-tools", help="wasm-tools executable")
    parser.add_argument("--manifest", default=str(DEFAULT_MANIFEST), help="workload corpus manifest")
    parser.add_argument("--repetitions", type=int, default=5, help="recorded samples per cell (default 5)")
    parser.add_argument("--warmups", type=int, default=1, help="unrecorded warm-up runs per cell (default 1)")
    parser.add_argument("--out", default="performance.json", help="output JSON path")
    return parser.parse_args(argv)


def main(argv=None):
    args = parse_args(argv)
    if args.repetitions < 1 or args.warmups < 0:
        print("performance measurement: error: repetitions must be >= 1 and warmups >= 0", file=sys.stderr)
        return 1
    binaries = {
        "default-interp": Path(args.default_bin),
        "jit-interp": Path(args.jit_bin),
        "jit-jit": Path(args.jit_bin),
    }
    for cell, binary in binaries.items():
        if not binary.is_file():
            print(f"performance measurement: error: no binary for cell {cell} at {binary}", file=sys.stderr)
            return 1
    work_dir = Path(args.out).resolve().parent / "perf-gate-workloads"
    work_dir.mkdir(parents=True, exist_ok=True)
    try:
        workloads = load_workloads(args.manifest)
        artifacts = {
            workload["name"]: prepare_workload(workload, work_dir, args.wasm_tools)
            for workload in workloads
        }
        record = measure(binaries, workloads, artifacts, args.repetitions, args.warmups)
    except MeasureError as error:
        print(f"performance measurement: error: {error}", file=sys.stderr)
        return 1
    Path(args.out).write_text(json.dumps(record, indent=2) + "\n", encoding="utf-8")
    print(f"performance measurement: wrote {args.out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
