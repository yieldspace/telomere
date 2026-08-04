"""Unit tests for the reusable interpreter-baseline measurement helpers."""

import math
import sys
import tempfile
import unittest
from pathlib import Path

TOOLS_DIR = Path(__file__).resolve().parent
if str(TOOLS_DIR) not in sys.path:
    sys.path.insert(0, str(TOOLS_DIR))

from _measure_support import (
    MeasurementError,
    below_noise_floor,
    coremark_score,
    counterbalanced_schedule,
    noise_floor,
    paired_slopes,
    sha256_file,
    symmetric_relative_contrast,
    williams_schedule,
)


class CounterbalancedScheduleTests(unittest.TestCase):
    def test_seeded_schedule_is_reproducible_and_complete(self):
        arms = ["default-a", "default-b", "opt-on", "opt-off"]
        first = counterbalanced_schedule(arms, rounds=8, seed=184)
        second = counterbalanced_schedule(arms, rounds=8, seed=184)

        self.assertEqual(first, second)
        self.assertEqual(len(first), 8)
        for order in first:
            self.assertEqual(sorted(order), sorted(arms))

    def test_schedule_rejects_duplicate_or_empty_arm_labels(self):
        with self.assertRaises(MeasurementError):
            counterbalanced_schedule([], rounds=1, seed=1)
        with self.assertRaises(MeasurementError):
            counterbalanced_schedule(["a", "a"], rounds=1, seed=1)


class WilliamsScheduleTests(unittest.TestCase):
    def setUp(self):
        self.arms = [
            "default-a",
            "default-b",
            "jit",
            "opt-on",
            "opt-off",
            "jit-opt-on",
            "jit-opt-off",
        ]

    def test_seven_arm_full_cycle_has_exact_position_and_carryover_balance(self):
        plan = williams_schedule(self.arms, rounds=14, seed=184)
        schedule = plan["schedule"]
        metadata = plan["metadata"]
        audit = metadata["balance_audit"]

        self.assertEqual(len(schedule), 14)
        self.assertEqual(metadata["cycle_rows"], 14)
        self.assertEqual(metadata["full_cycles"], 1)
        self.assertEqual(metadata["residual_rounds"], 0)
        self.assertEqual(metadata["carryover_scope"], "within_round")
        self.assertEqual(audit["carryover_scope"], "within_round")
        for arm in self.arms:
            self.assertEqual(
                set(audit["positions"]["counts"][arm].values()), {2}
            )
            self.assertEqual(
                set(audit["within_round_directed_carryover"]["counts"][arm].values()),
                {2},
            )
        self.assertTrue(audit["positions"]["exactly_balanced"])
        self.assertTrue(
            audit["within_round_directed_carryover"]["exactly_balanced"]
        )

    def test_fifteenth_round_is_explicit_seeded_residual_with_boundary_audit(self):
        first = williams_schedule(self.arms, rounds=15, seed=184)
        second = williams_schedule(self.arms, rounds=15, seed=184)
        metadata = first["metadata"]
        audit = metadata["balance_audit"]

        self.assertEqual(first, second)
        self.assertEqual(metadata["full_cycles"], 1)
        self.assertEqual(metadata["residual_rounds"], 1)
        self.assertEqual(len(metadata["residual_row_indices"]), 1)
        self.assertEqual(audit["residual"]["round_indices"], [14])
        self.assertEqual(len(audit["residual"]["internal_directed_carryover"]), 1)
        self.assertEqual(
            len(audit["residual"]["round_boundary_directed_carryover"]), 1
        )
        boundary = audit["round_boundary_directed_carryover"]
        self.assertFalse(boundary["exact_balance_claimed"])
        self.assertEqual(len(boundary["transitions"]), 14)
        self.assertEqual(boundary["imbalance"], boundary["max"] - boundary["min"])

    def test_incomplete_schedule_is_deterministic_and_marked_as_residual(self):
        first = williams_schedule(self.arms, rounds=3, seed=184)
        second = williams_schedule(self.arms, rounds=3, seed=184)

        self.assertEqual(first, second)
        metadata = first["metadata"]
        self.assertEqual(metadata["full_cycles"], 0)
        self.assertEqual(metadata["residual_rounds"], 3)
        self.assertEqual(len(metadata["residual_row_indices"]), 3)

    def test_seed_selects_the_row_order_without_changing_full_cycle_balance(self):
        first = williams_schedule(self.arms, rounds=14, seed=184)
        second = williams_schedule(self.arms, rounds=14, seed=185)

        self.assertNotEqual(
            first["metadata"]["cycle_row_order"],
            second["metadata"]["cycle_row_order"],
        )
        for plan in (first, second):
            audit = plan["metadata"]["balance_audit"]
            self.assertTrue(audit["positions"]["exactly_balanced"])
            self.assertTrue(
                audit["within_round_directed_carryover"]["exactly_balanced"]
            )


class CoremarkTests(unittest.TestCase):
    def test_parses_validated_iterations_per_second(self):
        output = (
            "2K performance run parameters for coremark.\n"
            "Correct operation validated.\n"
            "Iterations/Sec   : 1,234.50\n"
        )
        self.assertEqual(coremark_score(output), 1234.5)

    def test_rejects_unvalidated_or_ambiguous_output(self):
        with self.assertRaises(MeasurementError):
            coremark_score("Iterations/Sec   : 42\n")
        with self.assertRaises(MeasurementError):
            coremark_score(
                "Correct operation validated\n"
                "Iterations/Sec : 42\n"
                "Iterations/Sec : 43\n"
            )


class NoiseFloorTests(unittest.TestCase):
    def test_published_floor_records_bootstrap_and_empirical_bounds(self):
        contrasts = [-0.10, -0.08, -0.05, -0.03, -0.02, 0.01, 0.02, 0.03, 0.05, 0.09]
        first = noise_floor(contrasts, seed=184, bootstrap_resamples=100)
        second = noise_floor(contrasts, seed=184, bootstrap_resamples=100)

        self.assertTrue(first["publishable"])
        self.assertEqual(first, second)
        self.assertGreaterEqual(first["floor"], first["empirical_p95"])
        self.assertGreaterEqual(
            first["floor"], first["bootstrap_percentile_ci_95"]["upper"]
        )

    def test_small_sample_rules_do_not_claim_a_publishable_interval(self):
        quick = noise_floor([0.1, -0.2, 0.3], seed=1)
        self.assertFalse(quick["publishable"])
        self.assertEqual(quick["method"], "small_sample_max_abs")
        self.assertEqual(quick["floor"], 0.3)

        insufficient = noise_floor([0.1, -0.2], seed=1)
        self.assertIsNone(insufficient["floor"])
        self.assertEqual(
            insufficient["invalid_reason"], "insufficient_samples_for_interval"
        )

    def test_below_noise_floor_includes_equality(self):
        self.assertTrue(below_noise_floor(0.1, 0.1))
        self.assertTrue(below_noise_floor(-0.1, 0.1))
        self.assertFalse(below_noise_floor(0.100001, 0.1))


class SlopeTests(unittest.TestCase):
    def test_paired_slopes_preserve_rounds_constant_and_linearity(self):
        result = paired_slopes(
            10,
            [100.0, 110.0, 120.0],
            [200.0, 210.0, 220.0],
            [300.0, 310.0, 320.0],
            linearity_floor=0.0,
        )

        self.assertEqual(result["slope"], 10.0)
        self.assertEqual(result["constant_term"], 10.0)
        self.assertEqual(result["paired_deltas_n_to_2n"], [100.0, 100.0, 100.0])
        self.assertTrue(result["linearity"]["is_linear"])
        self.assertIsNone(result["invalid_reason"])

    def test_non_positive_or_non_finite_increment_never_passes_linearity(self):
        non_positive = paired_slopes(
            10,
            [100.0],
            [100.0],
            [300.0],
            linearity_floor=0.1,
        )
        self.assertEqual(non_positive["invalid_reason"], "non_positive_increment")
        self.assertIsNone(non_positive["slope"])

        non_finite = paired_slopes(
            10,
            [100.0],
            [math.inf],
            [300.0],
            linearity_floor=0.1,
        )
        self.assertEqual(non_finite["invalid_reason"], "non_finite_increment")
        self.assertIsNone(non_finite["slope"])

    def test_symmetric_contrast_has_no_asymmetric_reference_arm(self):
        self.assertAlmostEqual(symmetric_relative_contrast(3.0, 1.0), 1.0)
        self.assertAlmostEqual(symmetric_relative_contrast(1.0, 3.0), -1.0)


class HashTests(unittest.TestCase):
    def test_sha256_file_streams_known_content(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "artifact"
            path.write_bytes(b"telomere")
            self.assertEqual(
                sha256_file(path),
                "9dd32d4b577cf69c208e3e5f718ae341c7399e50498720b85f834812120b9cc3",
            )


if __name__ == "__main__":
    unittest.main()
