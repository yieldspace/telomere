#!/usr/bin/env python3
"""Unit tests for the JIT test coverage checker."""

from __future__ import annotations

import importlib.util
import sys
import unittest
from pathlib import Path


TOOLS_DIR = Path(__file__).resolve().parent
MODULE_PATH = TOOLS_DIR / "check-jit-test-coverage.py"
SPEC = importlib.util.spec_from_file_location("check_jit_test_coverage", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
jit_coverage = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = jit_coverage
SPEC.loader.exec_module(jit_coverage)


class SourceDetectionTests(unittest.TestCase):
    def test_single_gated_test_is_detected(self) -> None:
        source = (
            '#[cfg(feature = "jit")]\n'
            "#[tokio::test]\n"
            "async fn jit_only_test() {}\n"
        )
        self.assertTrue(jit_coverage.source_has_jit_gated_tests(source))

    def test_multiline_cfg_gate_is_detected(self) -> None:
        source = (
            '#[cfg(all(\n    feature = "jit",\n    target_arch = "x86_64"\n))]\n'
            "fn helper() {}\n\n#[test]\nfn some_test() {}\n"
        )
        self.assertTrue(jit_coverage.source_has_jit_gated_tests(source))

    def test_ungated_tests_are_not_detected(self) -> None:
        source = '#[cfg(feature = "threads")]\n#[test]\nfn threaded_test() {}\n'
        self.assertFalse(jit_coverage.source_has_jit_gated_tests(source))

    def test_jit_mention_without_tests_is_not_detected(self) -> None:
        source = '#[cfg(feature = "jit")]\npub fn jit_helper() {}\n'
        self.assertFalse(jit_coverage.source_has_jit_gated_tests(source))


class ExecutedTargetsTests(unittest.TestCase):
    def test_named_targets_with_jit_are_covered(self) -> None:
        ci = (
            "      - name: JIT regression tests\n"
            "        run: cargo test -p telomere --release --features jit --test jit\n"
            "      - name: JIT-gated tests\n"
            "        run: cargo test -p telomere --release --features jit "
            "--test trap_info --test metering\n"
        )
        covered, covers_all = jit_coverage.executed_jit_targets(ci)
        self.assertEqual(covered, {"jit", "trap_info", "metering"})
        self.assertFalse(covers_all)

    def test_step_without_jit_feature_does_not_cover(self) -> None:
        ci = "        run: cargo test --verbose --release --workspace\n"
        covered, covers_all = jit_coverage.executed_jit_targets(ci)
        self.assertEqual(covered, set())
        self.assertFalse(covers_all)

    def test_feature_list_containing_jit_covers(self) -> None:
        ci = '        run: cargo test -p telomere --features "simd,jit" --test trap_info\n'
        covered, covers_all = jit_coverage.executed_jit_targets(ci)
        self.assertEqual(covered, {"trap_info"})
        self.assertFalse(covers_all)

    def test_unrestricted_jit_step_covers_all(self) -> None:
        ci = "        run: cargo test -p telomere --release --features jit\n"
        covered, covers_all = jit_coverage.executed_jit_targets(ci)
        self.assertEqual(covered, set())
        self.assertTrue(covers_all)

    def test_cargo_run_does_not_cover(self) -> None:
        ci = '          output="$(cargo run --quiet --features jit -- --jit a.wasm)"\n'
        covered, covers_all = jit_coverage.executed_jit_targets(ci)
        self.assertEqual(covered, set())
        self.assertFalse(covers_all)


class CoverageErrorsTests(unittest.TestCase):
    def test_missing_target_is_reported(self) -> None:
        gated = [("telomere", "trap_info", Path("tests/trap_info.rs"))]
        errors = jit_coverage.coverage_errors(gated, {"jit"}, False)
        self.assertEqual(len(errors), 1)
        self.assertIn("trap_info", errors[0])

    def test_covered_target_is_not_reported(self) -> None:
        gated = [("telomere", "trap_info", Path("tests/trap_info.rs"))]
        errors = jit_coverage.coverage_errors(gated, {"jit", "trap_info"}, False)
        self.assertEqual(errors, [])

    def test_covers_all_suppresses_errors(self) -> None:
        gated = [("telomere", "trap_info", Path("tests/trap_info.rs"))]
        errors = jit_coverage.coverage_errors(gated, set(), True)
        self.assertEqual(errors, [])


if __name__ == "__main__":
    unittest.main()
