#!/usr/bin/env python3
"""Gate CI performance measurements against a recorded baseline.

Issue #164 gated binary size but nothing gated speed, and three real
regressions (a 35% interpreter slowdown, an up-to-2x JIT-feature tax, and
the continuation-bridge crossing cost) were all found by hand. Issue #212
adds the speed twin, consuming the methodology of
docs/core/interpreter-baseline.md: tools/measure-performance.py measures
the methodology's L2 corpus in its default/jit build matrix through
telomere-cli, and this checker compares the record against
tools/perf-baseline.json.

What is gated, and what is not, follows the observed evidence (five
same-tree CI runs, recorded in docs/benchmarks/performance.md). The
GitHub-hosted ubuntu-24.04 fleet is heterogeneous: the five runs landed
on five different CPU models, and identical code's absolute medians
varied by 60-200% between runs purely from host allocation. An absolute
wall-time ceiling would fail pull requests for the runner they happened
to draw, so absolute medians are recorded and reported as advisory
warnings only. Ratios computed inside one run on one host are the
fleet-invariant quantities, and those are what this gate enforces:

- ``jit_feature_tax_ratio`` (jit-build interpreter median / default-build
  interpreter median) — catches the JIT-feature tax, the regression a
  single-configuration gate cannot see at all;
- ``jit_speed_ratio`` (JIT median / jit-build interpreter median) —
  catches the JIT losing ground against the interpreter on the same
  host, the aggregate signature of the bridged-crossing regression.

The ceilings are conservative policy margins over the recorded
baselines, not derived optima: #184 established that a threshold
calibrated from an A/A noise floor is circular, because the floor cannot
observe layout-dependent harm and a floor observed under contention
launders that contention into the definition of acceptable. Each
ceiling's margin, the observed spread it rests on, and the regressions
it does and does not catch are documented in
docs/benchmarks/performance.md.

This checker is stdlib-only, in the style of tools/check-embedder-size.py.
A gated series that exceeds its ceiling fails, naming the measured
value, the ceiling, the baseline, and the baseline source. A missing
series fails closed. An absolute median beyond its advisory ceiling is
a warning: it never fails the gate, because on this fleet it is as
likely to be the runner as the code.
"""

import argparse
import json
import sys
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_BASELINE = REPO_ROOT / "tools" / "perf-baseline.json"
DEFAULT_MEASUREMENT = "performance.json"

CELLS = ("default-interp", "jit-interp", "jit-jit")
RATIO_SERIES = ("jit_feature_tax_ratio", "jit_speed_ratio")


class GateError(RuntimeError):
    """A baseline or measurement file is missing, malformed, or incomplete."""


def load_json(path):
    try:
        text = Path(path).read_text(encoding="utf-8")
    except OSError as error:
        raise GateError(f"could not read {path}: {error}") from error
    try:
        return json.loads(text)
    except json.JSONDecodeError as error:
        raise GateError(f"invalid JSON in {path}: {error}") from error


def _is_number(value):
    return isinstance(value, (int, float)) and not isinstance(value, bool)


def check(baseline, measurement):
    """Return (failures, warnings); empty failures means the gate passes."""
    if not isinstance(baseline, dict):
        raise GateError("performance baseline is not a JSON object")
    source = baseline.get("source")
    if not isinstance(source, dict) or not isinstance(source.get("summary"), str):
        raise GateError("performance baseline lacks source.summary")
    thresholds = baseline.get("thresholds")
    if not isinstance(thresholds, dict) or not thresholds:
        raise GateError("performance baseline lacks thresholds")
    if not isinstance(measurement, dict):
        raise GateError("performance measurement is not a JSON object")
    cells_by_workload = measurement.get("cells")
    derived_by_workload = measurement.get("derived")
    if not isinstance(cells_by_workload, dict) or not isinstance(derived_by_workload, dict):
        raise GateError(
            "performance measurement lacks cells/derived; "
            "pass the JSON produced by tools/measure-performance.py"
        )

    failures = []
    warnings = []
    for workload in sorted(thresholds):
        entry = thresholds[workload]
        if not isinstance(entry, dict) or not isinstance(entry.get("cells"), dict):
            raise GateError(f"performance baseline thresholds for {workload} are malformed")
        measured_cells = cells_by_workload.get(workload)
        for cell in CELLS:
            limits = entry["cells"].get(cell)
            if not isinstance(limits, dict) or not _is_number(limits.get("baseline_median_ns")) \
                    or not _is_number(limits.get("advisory_ceiling_ns")):
                raise GateError(
                    f"performance baseline entry for {workload}/{cell} "
                    "lacks numeric baseline_median_ns/advisory_ceiling_ns"
                )
            if not isinstance(measured_cells, dict) or not isinstance(measured_cells.get(cell), dict) \
                    or not _is_number(measured_cells[cell].get("median_ns")):
                failures.append(
                    f"performance gate: no measurement for {workload}/{cell}; "
                    "the performance run did not produce this series"
                )
                continue
            value = measured_cells[cell]["median_ns"]
            if value > limits["advisory_ceiling_ns"]:
                warnings.append(
                    f"performance gate: warning: {workload}/{cell} median wall time "
                    f"measured {value} ns exceeds the advisory ceiling "
                    f"{limits['advisory_ceiling_ns']} ns (baseline "
                    f"{limits['baseline_median_ns']} ns from {source['summary']}); "
                    "absolute times are not gated on the heterogeneous CI fleet — "
                    "treat this as a prompt to look, not as a verdict; see "
                    "docs/benchmarks/performance.md"
                )
        measured_derived = derived_by_workload.get(workload)
        for series in RATIO_SERIES:
            limits = entry.get(series)
            if not isinstance(limits, dict) or not _is_number(limits.get("baseline")) \
                    or not _is_number(limits.get("ceiling")):
                raise GateError(
                    f"performance baseline entry for {workload} lacks a numeric "
                    f"{series} baseline/ceiling"
                )
            ratio = None
            if isinstance(measured_derived, dict) and isinstance(measured_derived.get(series), dict):
                candidate = measured_derived[series].get("value")
                if _is_number(candidate):
                    ratio = candidate
            if ratio is None:
                failures.append(
                    f"performance gate: no measurement for {workload}/{series}; "
                    "the performance run did not produce this series"
                )
            elif ratio > limits["ceiling"]:
                failures.append(
                    f"performance gate: {workload} {series} measured {ratio:.4f} "
                    f"exceeds ceiling {limits['ceiling']:.4f} "
                    f"(baseline {limits['baseline']:.4f} from {source['summary']}; "
                    "if this slowdown is intentional, re-baseline as documented "
                    "in docs/benchmarks/performance.md)"
                )
    return failures, warnings


def parse_args(argv=None):
    parser = argparse.ArgumentParser(
        description="fail when a measured performance ratio exceeds its baseline ceiling"
    )
    parser.add_argument(
        "--baseline",
        default=str(DEFAULT_BASELINE),
        help="performance baseline JSON (default: tools/perf-baseline.json)",
    )
    parser.add_argument(
        "measurement",
        nargs="?",
        default=DEFAULT_MEASUREMENT,
        help="performance measurement JSON (default: performance.json in the working directory)",
    )
    return parser.parse_args(argv)


def main(argv=None):
    args = parse_args(argv)
    try:
        baseline = load_json(args.baseline)
        measurement = load_json(args.measurement)
        failures, warnings = check(baseline, measurement)
    except GateError as error:
        print(f"performance gate: error: {error}", file=sys.stderr)
        return 1
    for warning in warnings:
        print(warning, file=sys.stderr)
    if failures:
        for failure in failures:
            print(failure, file=sys.stderr)
        return 1
    print("performance gate: all gated ratios are within their ceilings")
    return 0


if __name__ == "__main__":
    sys.exit(main())
