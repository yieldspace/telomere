#!/usr/bin/env python3
"""Gate CI performance measurements against a recorded baseline.

Issue #164 gated binary size but nothing gated speed, and three real
regressions (a 35% interpreter slowdown, an up-to-2x JIT-feature tax, and
the continuation-bridge crossing cost) were all found by hand. Issue #212
adds the speed twin, consuming the methodology of
docs/core/interpreter-baseline.md: tools/measure-performance.py measures
the methodology's L2 corpus in its default/jit build matrix through
telomere-cli, and this checker compares the medians against
tools/perf-baseline.json.

The thresholds are conservative absolute ceilings, not derived optima.
#184 established that a threshold calibrated from an A/A noise floor is
circular — the floor cannot observe layout-dependent harm, and a floor
observed on a contaminated machine launders that contamination into the
definition of acceptable. So the baseline records the observed CI medians
and their run-to-run spread (see docs/benchmarks/performance.md), and each
ceiling is a policy margin above the baseline: wide enough that the
observed spread cannot trip it, narrow enough that the smallest
regression this gate exists to catch (35%) cannot pass under it. The
ceiling is provisional and is described that way in the docs; it is not
lowered without evidence, and the environment is recorded with every
baseline.

This checker is stdlib-only, in the style of tools/check-embedder-size.py.
It fails when a measured median exceeds its ceiling, naming the measured
value, the ceiling, the baseline, and the baseline source in every
failure message. A missing series fails closed.
"""

import argparse
import json
import sys
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_BASELINE = REPO_ROOT / "tools" / "perf-baseline.json"
DEFAULT_MEASUREMENT = "performance.json"

CELLS = ("default-interp", "jit-interp", "jit-jit")
RATIO_NAME = "jit_feature_tax_ratio"


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
    """Return a list of failure strings; an empty list means the gate passes."""
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
    for workload in sorted(thresholds):
        entry = thresholds[workload]
        if not isinstance(entry, dict) or not isinstance(entry.get("cells"), dict):
            raise GateError(f"performance baseline thresholds for {workload} are malformed")
        measured_cells = cells_by_workload.get(workload)
        for cell in CELLS:
            limits = entry["cells"].get(cell)
            if not isinstance(limits, dict) or not _is_number(limits.get("baseline_median_ns")) \
                    or not _is_number(limits.get("ceiling_median_ns")):
                raise GateError(
                    f"performance baseline entry for {workload}/{cell} "
                    "lacks numeric baseline_median_ns/ceiling_median_ns"
                )
            if not isinstance(measured_cells, dict) or not isinstance(measured_cells.get(cell), dict) \
                    or not _is_number(measured_cells[cell].get("median_ns")):
                failures.append(
                    f"performance gate: no measurement for {workload}/{cell}; "
                    "the performance run did not produce this series"
                )
                continue
            value = measured_cells[cell]["median_ns"]
            if value > limits["ceiling_median_ns"]:
                failures.append(
                    f"performance gate: {workload}/{cell} median wall time measured "
                    f"{value} ns exceeds ceiling {limits['ceiling_median_ns']} ns "
                    f"(baseline {limits['baseline_median_ns']} ns from {source['summary']}; "
                    "if this slowdown is intentional, re-baseline as documented "
                    "in docs/benchmarks/performance.md)"
                )
        ratio_limits = entry.get(RATIO_NAME)
        if not isinstance(ratio_limits, dict) or not _is_number(ratio_limits.get("baseline")) \
                or not _is_number(ratio_limits.get("ceiling")):
            raise GateError(
                f"performance baseline entry for {workload} lacks a numeric {RATIO_NAME} baseline/ceiling"
            )
        measured_derived = derived_by_workload.get(workload)
        ratio = None
        if isinstance(measured_derived, dict) and isinstance(measured_derived.get(RATIO_NAME), dict):
            candidate = measured_derived[RATIO_NAME].get("value")
            if _is_number(candidate):
                ratio = candidate
        if ratio is None:
            failures.append(
                f"performance gate: no measurement for {workload}/{RATIO_NAME}; "
                "the performance run did not produce this series"
            )
        elif ratio > ratio_limits["ceiling"]:
            failures.append(
                f"performance gate: {workload} {RATIO_NAME} measured {ratio:.4f} "
                f"exceeds ceiling {ratio_limits['ceiling']:.4f} "
                f"(baseline {ratio_limits['baseline']:.4f} from {source['summary']}; "
                "the jit-feature build's interpreter cell slowed relative to the "
                "default build's; if this tax is intentional, re-baseline as "
                "documented in docs/benchmarks/performance.md)"
            )
    return failures


def parse_args(argv=None):
    parser = argparse.ArgumentParser(
        description="fail when a measured performance median exceeds its baseline ceiling"
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
        failures = check(baseline, measurement)
    except GateError as error:
        print(f"performance gate: error: {error}", file=sys.stderr)
        return 1
    if failures:
        for failure in failures:
            print(failure, file=sys.stderr)
        return 1
    print("performance gate: all measured medians are within their ceilings")
    return 0


if __name__ == "__main__":
    sys.exit(main())
