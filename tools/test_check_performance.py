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


def measurement(default_ns, jit_interp_ns, jit_jit_ns, ratio=None):
    if ratio is None:
        ratio = jit_interp_ns / default_ns
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
                "jit_feature_tax_ratio": {"value": ratio},
            }
        },
    }


def baseline(cell_ceiling, ratio_ceiling, cell_baseline=100, ratio_baseline=1.0):
    return {
        "source": {"summary": "unit-test baseline"},
        "thresholds": {
            "loop-linear": {
                "cells": {
                    cell: {
                        "baseline_median_ns": cell_baseline,
                        "ceiling_median_ns": cell_ceiling,
                    }
                    for cell in ("default-interp", "jit-interp", "jit-jit")
                },
                "jit_feature_tax_ratio": {
                    "baseline": ratio_baseline,
                    "ceiling": ratio_ceiling,
                },
            }
        },
    }


class GateTests(unittest.TestCase):
    def test_medians_at_ceiling_pass(self) -> None:
        failures = perf_gate.check(baseline(125, 1.25), measurement(125, 125, 125, ratio=1.0))
        self.assertEqual(failures, [])

    def test_median_above_ceiling_fails_and_names_values(self) -> None:
        failures = perf_gate.check(baseline(125, 1.25), measurement(126, 100, 100, ratio=1.0))
        self.assertEqual(len(failures), 1)
        self.assertIn("loop-linear/default-interp", failures[0])
        self.assertIn("126 ns", failures[0])
        self.assertIn("125 ns", failures[0])
        self.assertIn("unit-test baseline", failures[0])
        self.assertIn("docs/benchmarks/performance.md", failures[0])

    def test_ratio_above_ceiling_fails_even_when_cells_pass(self) -> None:
        # Both interpreter medians are inside their absolute ceilings, but
        # the jit build's interpreter cell drifted relative to the default
        # build's: the feature-tax series exists to catch exactly this.
        failures = perf_gate.check(
            baseline(1000, 1.25), measurement(100, 130, 100, ratio=1.30)
        )
        self.assertEqual(len(failures), 1)
        self.assertIn("jit_feature_tax_ratio", failures[0])
        self.assertIn("1.3000", failures[0])
        self.assertIn("1.2500", failures[0])

    def test_missing_cell_fails_closed(self) -> None:
        measured = measurement(100, 100, 100)
        del measured["cells"]["loop-linear"]["jit-jit"]
        failures = perf_gate.check(baseline(125, 1.25), measured)
        self.assertEqual(len(failures), 1)
        self.assertIn("no measurement for loop-linear/jit-jit", failures[0])

    def test_missing_workload_fails_closed(self) -> None:
        measured = measurement(100, 100, 100)
        measured["cells"] = {}
        measured["derived"] = {}
        failures = perf_gate.check(baseline(125, 1.25), measured)
        self.assertEqual(len(failures), 4)  # three cells plus the ratio

    def test_missing_ratio_fails_closed(self) -> None:
        measured = measurement(100, 100, 100)
        measured["derived"]["loop-linear"] = {}
        failures = perf_gate.check(baseline(125, 1.25), measured)
        self.assertEqual(len(failures), 1)
        self.assertIn("jit_feature_tax_ratio", failures[0])

    def test_malformed_baseline_raises(self) -> None:
        with self.assertRaises(perf_gate.GateError):
            perf_gate.check({"thresholds": {}}, measurement(1, 1, 1))
        with self.assertRaises(perf_gate.GateError):
            perf_gate.check({"source": {"summary": "x"}, "thresholds": {}}, measurement(1, 1, 1))

    def test_malformed_measurement_raises(self) -> None:
        with self.assertRaises(perf_gate.GateError):
            perf_gate.check(baseline(125, 1.25), {"cells": {}})

    def test_baseline_entry_missing_ceiling_raises(self) -> None:
        broken = baseline(125, 1.25)
        del broken["thresholds"]["loop-linear"]["cells"]["jit-interp"]["ceiling_median_ns"]
        with self.assertRaises(perf_gate.GateError):
            perf_gate.check(broken, measurement(100, 100, 100))


if __name__ == "__main__":
    unittest.main()
