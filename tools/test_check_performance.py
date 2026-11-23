#!/usr/bin/env python3
"""Unit tests for the performance regression gate checker."""

from __future__ import annotations

import importlib.util
import sys
import unittest
from pathlib import Path


TOOLS_DIR = Path(__file__).resolve().parent
MODULE_PATH = TOOLS_DIR / "check-performance.py"
SPEC = importlib.util.spec_from_file_location("check_performance", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
perf_gate = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = perf_gate
SPEC.loader.exec_module(perf_gate)


def measurement(default_ns, jit_interp_ns, jit_jit_ns, tax=None, speed=None):
    if tax is None:
        tax = jit_interp_ns / default_ns
    if speed is None:
        speed = jit_jit_ns / jit_interp_ns
    return {
        "cells": {
            "loop-linear": {
                "default-interp": {"median_ns": default_ns},
                "jit-interp": {"median_ns": jit_interp_ns},
                "jit-jit": {"median_ns": jit_jit_ns},
            }
        },
        "derived": {
            "loop-linear": {
                "jit_feature_tax_ratio": {"value": tax},
                "jit_speed_ratio": {"value": speed},
            }
        },
    }


def baseline(advisory=125, tax_ceiling=1.25, speed_ceiling=1.5):
    return {
        "source": {"summary": "unit-test baseline"},
        "thresholds": {
            "loop-linear": {
                "cells": {
                    cell: {
                        "baseline_median_ns": 100,
                        "advisory_ceiling_ns": advisory,
                    }
                    for cell in ("default-interp", "jit-interp", "jit-jit")
                },
                "jit_feature_tax_ratio": {"baseline": 1.0, "ceiling": tax_ceiling},
                "jit_speed_ratio": {"baseline": 1.0, "ceiling": speed_ceiling},
            }
        },
    }


class GateTests(unittest.TestCase):
    def test_ratios_at_ceiling_pass_without_warnings(self) -> None:
        failures, warnings = perf_gate.check(
            baseline(), measurement(100, 100, 100, tax=1.25, speed=1.5)
        )
        self.assertEqual(failures, [])
        self.assertEqual(warnings, [])

    def test_tax_ratio_above_ceiling_fails_and_names_values(self) -> None:
        failures, _ = perf_gate.check(
            baseline(), measurement(100, 100, 100, tax=1.26, speed=1.0)
        )
        self.assertEqual(len(failures), 1)
        self.assertIn("jit_feature_tax_ratio", failures[0])
        self.assertIn("1.2600", failures[0])
        self.assertIn("1.2500", failures[0])
        self.assertIn("unit-test baseline", failures[0])
        self.assertIn("docs/benchmarks/performance.md", failures[0])

    def test_speed_ratio_above_ceiling_fails(self) -> None:
        failures, _ = perf_gate.check(
            baseline(), measurement(100, 100, 100, tax=1.0, speed=1.51)
        )
        self.assertEqual(len(failures), 1)
        self.assertIn("jit_speed_ratio", failures[0])

    def test_absolute_slowdown_warns_but_does_not_fail(self) -> None:
        # Every cell is beyond its advisory ceiling (a slow runner, or a
        # uniform slowdown), but the within-run ratios are unchanged:
        # on the heterogeneous CI fleet this must not fail the gate.
        failures, warnings = perf_gate.check(
            baseline(), measurement(1000, 1000, 1000, tax=1.0, speed=1.0)
        )
        self.assertEqual(failures, [])
        self.assertEqual(len(warnings), 3)
        self.assertIn("warning", warnings[0])
        self.assertIn("not gated", warnings[0])

    def test_missing_cell_fails_closed(self) -> None:
        measured = measurement(100, 100, 100)
        del measured["cells"]["loop-linear"]["jit-jit"]
        failures, _ = perf_gate.check(baseline(), measured)
        self.assertEqual(len(failures), 1)
        self.assertIn("no measurement for loop-linear/jit-jit", failures[0])

    def test_missing_workload_fails_closed(self) -> None:
        measured = measurement(100, 100, 100)
        measured["cells"] = {}
        measured["derived"] = {}
        failures, _ = perf_gate.check(baseline(), measured)
        self.assertEqual(len(failures), 5)  # three cells plus two ratio series

    def test_missing_ratio_fails_closed(self) -> None:
        measured = measurement(100, 100, 100)
        del measured["derived"]["loop-linear"]["jit_speed_ratio"]
        failures, _ = perf_gate.check(baseline(), measured)
        self.assertEqual(len(failures), 1)
        self.assertIn("jit_speed_ratio", failures[0])

    def test_malformed_baseline_raises(self) -> None:
        with self.assertRaises(perf_gate.GateError):
            perf_gate.check({"thresholds": {}}, measurement(1, 1, 1))
        with self.assertRaises(perf_gate.GateError):
            perf_gate.check({"source": {"summary": "x"}, "thresholds": {}}, measurement(1, 1, 1))

    def test_malformed_measurement_raises(self) -> None:
        with self.assertRaises(perf_gate.GateError):
            perf_gate.check(baseline(), {"cells": {}})

    def test_baseline_entry_missing_ceiling_raises(self) -> None:
        broken = baseline()
        del broken["thresholds"]["loop-linear"]["jit_feature_tax_ratio"]["ceiling"]
        with self.assertRaises(perf_gate.GateError):
            perf_gate.check(broken, measurement(100, 100, 100))


if __name__ == "__main__":
    unittest.main()
