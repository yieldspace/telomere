#!/usr/bin/env python3
"""Unit tests for the embedder size regression gate checker."""

from __future__ import annotations

import importlib.util
import sys
import unittest
from pathlib import Path


TOOLS_DIR = Path(__file__).resolve().parent
MODULE_PATH = TOOLS_DIR / "check-embedder-size.py"
SPEC = importlib.util.spec_from_file_location("check_embedder_size", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
size_gate = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = size_gate
SPEC.loader.exec_module(size_gate)


def measurement(profile, config, file_bytes, text_bytes):
    return {
        "profile": profile,
        "configs": [
            {
                "config": config,
                "artifact": {
                    "stripped_file_bytes": file_bytes,
                    "stripped_text_section": {"bytes": text_bytes},
                },
            }
        ],
    }


def baseline(file_threshold, text_threshold, file_baseline=None, text_baseline=None):
    return {
        "source": {"summary": "unit-test baseline"},
        "thresholds": {
            "release-size": {
                "core": {
                    "stripped_file_bytes": {
                        "baseline": file_threshold if file_baseline is None else file_baseline,
                        "threshold": file_threshold,
                    },
                    "stripped_text_bytes": {
                        "baseline": text_threshold if text_baseline is None else text_baseline,
                        "threshold": text_threshold,
                    },
                }
            }
        },
    }


class GateTests(unittest.TestCase):
    def test_sizes_at_threshold_pass(self) -> None:
        failures = size_gate.check(
            baseline(1000, 800),
            [measurement("release-size", "core", 1000, 800)],
        )
        self.assertEqual(failures, [])

    def test_sizes_below_threshold_pass(self) -> None:
        failures = size_gate.check(
            baseline(1000, 800),
            [measurement("release-size", "core", 900, 700)],
        )
        self.assertEqual(failures, [])

    def test_file_growth_fails_and_names_measured_threshold_and_source(self) -> None:
        failures = size_gate.check(
            baseline(1000, 800),
            [measurement("release-size", "core", 1001, 800)],
        )
        self.assertEqual(len(failures), 1)
        self.assertIn("measured 1001 B", failures[0])
        self.assertIn("threshold 1000 B", failures[0])
        self.assertIn("baseline 1000 B from unit-test baseline", failures[0])

    def test_text_growth_fails(self) -> None:
        failures = size_gate.check(
            baseline(1000, 800),
            [measurement("release-size", "core", 1000, 801)],
        )
        self.assertEqual(len(failures), 1)
        self.assertIn("stripped_text_bytes", failures[0])

    def test_missing_configuration_fails(self) -> None:
        failures = size_gate.check(
            baseline(1000, 800),
            [measurement("release-size", "wasi", 1, 1)],
        )
        self.assertEqual(len(failures), 1)
        self.assertIn("no measurement for release-size/core", failures[0])

    def test_missing_metric_fails(self) -> None:
        document = measurement("release-size", "core", 1000, 800)
        del document["configs"][0]["artifact"]["stripped_text_section"]
        failures = size_gate.check(baseline(1000, 800), [document])
        self.assertEqual(len(failures), 1)
        self.assertIn("lacks stripped_text_bytes", failures[0])

    def test_malformed_measurement_raises(self) -> None:
        with self.assertRaises(size_gate.GateError):
            size_gate.check(baseline(1000, 800), [{"configs": []}])

    def test_malformed_baseline_raises(self) -> None:
        with self.assertRaises(size_gate.GateError):
            size_gate.check({"thresholds": {}}, [])


if __name__ == "__main__":
    unittest.main()
