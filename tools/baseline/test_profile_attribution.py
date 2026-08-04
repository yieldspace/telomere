#!/usr/bin/env python3
"""Regression tests for weighted profiler-attribution parsing."""

from __future__ import annotations

import sys
import unittest
from pathlib import Path


HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

from profile_attribution import (  # noqa: E402
    AttributionParseError,
    fold_families,
    parse_darwin_sample,
    parse_perf_report,
    table_rows,
)


FIXTURES = HERE / "fixtures"


class ProfileAttributionTests(unittest.TestCase):
    def test_darwin_sample_uses_leading_sample_count_as_weight(self) -> None:
        weights = parse_darwin_sample(
            (FIXTURES / "darwin-sample-weighted.txt").read_text(encoding="utf-8")
        )

        self.assertEqual(weights["op_i32_and"], 40.0)
        self.assertEqual(weights["op_i32_load_const_base"], 15.0)
        self.assertEqual(weights["op_call"], 5.0)
        self.assertEqual(sum(weights.values()), 60.0)

    def test_perf_report_uses_overhead_percentage_as_weight(self) -> None:
        weights = parse_perf_report(
            (FIXTURES / "linux-perf-weighted.txt").read_text(encoding="utf-8")
        )

        self.assertEqual(weights["op_i32_and"], 62.5)
        self.assertEqual(weights["op_i32_load_const_base"], 25.0)
        self.assertEqual(weights["op_call"], 12.5)

    def test_family_table_preserves_weighted_shares(self) -> None:
        weights = parse_darwin_sample(
            (FIXTURES / "darwin-sample-weighted.txt").read_text(encoding="utf-8")
        )
        families = fold_families(weights)

        self.assertEqual(families, {"Numeric": 40.0, "Memory": 15.0, "Call": 5.0})
        self.assertEqual(
            list(table_rows(families)),
            ["Numeric\t40\t66.67", "Memory\t15\t25.00", "Call\t5\t8.33"],
        )

    def test_unweighted_symbol_occurrences_fail_closed(self) -> None:
        with self.assertRaises(AttributionParseError):
            parse_darwin_sample("op_i32_and\n")
        with self.assertRaises(AttributionParseError):
            parse_perf_report("op_i32_and\n")


if __name__ == "__main__":
    unittest.main()
