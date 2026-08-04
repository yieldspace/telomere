"""Unit tests for interpreter-baseline orchestration and manifest contracts."""

import importlib.util
import json
import sys
import tempfile
import unittest
from unittest import mock
from pathlib import Path
from types import SimpleNamespace


TOOLS_DIR = Path(__file__).resolve().parent
if str(TOOLS_DIR) not in sys.path:
    sys.path.insert(0, str(TOOLS_DIR))

SPEC = importlib.util.spec_from_file_location(
    "measure_interpreter_baseline", TOOLS_DIR / "measure-interpreter-baseline.py"
)
assert SPEC is not None
assert SPEC.loader is not None
baseline = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(baseline)


class ArgumentAndBuildTests(unittest.TestCase):
    def test_compare_requires_baseline_and_build_modes_are_exclusive(self):
        with self.assertRaises(baseline.BaselineError) as missing:
            baseline.parse_args(["--mode", "compare"])
        self.assertEqual(missing.exception.reason, "missing_baseline")

        with self.assertRaises(baseline.BaselineError) as incompatible:
            baseline.parse_args(
                ["--mode", "first-record", "--build-only", "--skip-build"]
            )
        self.assertEqual(incompatible.exception.reason, "invalid_build_mode")

        build_only = baseline.parse_args(["--build-only"])
        self.assertIsNone(build_only.mode)

        with self.assertRaises(baseline.BaselineError) as non_publishable:
            baseline.parse_args(["--mode", "first-record", "--rounds", "9"])
        self.assertEqual(
            non_publishable.exception.reason,
            "non_publishable_rounds_require_quick",
        )
        quick = baseline.parse_args(
            ["--mode", "first-record", "--quick", "--rounds", "9"]
        )
        self.assertTrue(quick.quick)

    def test_cargo_build_command_and_skip_build_hash_contract(self):
        with tempfile.TemporaryDirectory() as temporary:
            target_dir = Path(temporary)
            copied = target_dir / "bin"
            copied.mkdir()
            manifest_builds = {}
            for name in baseline.BUILD_CONFIGS:
                binary = copied / name
                binary.write_bytes(name.encode("utf-8"))
                features = list(baseline.BUILD_CONFIGS[name]["features"])
                manifest_builds[name] = {
                    "build": name,
                    "features": features,
                    "cargo_command": baseline.cargo_build_command(
                        features, target_dir / "cargo"
                    ),
                    "copied_binary": str(binary),
                    "sha256": baseline.sha256_file(binary),
                    "witness_a": {
                        "status": "pass",
                        "features": features,
                        "command": baseline.witness_a_command(
                            features, target_dir / "cargo"
                        ),
                        "test": "release_call_loop_keeps_direct_threading",
                    },
                }
            baseline.atomic_write_json(
                baseline.build_manifest_path(target_dir),
                {
                    "schema_version": 1,
                    "source_commit": baseline.current_source_commit(),
                    "builds": manifest_builds,
                },
            )

            args = SimpleNamespace(target_dir=target_dir, skip_build=True)
            with mock.patch.object(baseline, "require_clean_tracked_worktree"):
                records = baseline.build_binaries(args)

            self.assertEqual(set(records), set(baseline.BUILD_CONFIGS))
            self.assertEqual(records["jit"]["features"], ["jit"])
            self.assertEqual(
                records["jit-opt"]["features"], ["jit", "measure-switches"]
            )
            self.assertEqual(
                records["default"]["build_action"],
                "reused_after_manifest_verification",
            )
            self.assertIn("--target-dir", records["default"]["cargo_command"])

    def test_skip_build_rejects_a_dirty_tracked_worktree_before_manifest_reuse(self):
        args = SimpleNamespace(target_dir=Path("unused"), skip_build=True)
        dirty = baseline.BaselineError(
            "dirty_tracked_worktree", "simulated dirty tracked worktree"
        )
        with mock.patch.object(
            baseline, "require_clean_tracked_worktree", side_effect=dirty
        ):
            with self.assertRaises(baseline.BaselineError) as rejected:
                baseline.build_binaries(args)

        self.assertEqual(rejected.exception.reason, "dirty_tracked_worktree")


class ManifestTests(unittest.TestCase):
    def test_manifest_has_pinned_artifacts_and_exact_newline_validation(self):
        workloads = baseline.load_manifest(baseline.DEFAULT_MANIFEST)
        self.assertEqual(
            [workload["name"] for workload in workloads],
            ["coremark", "loop-linear", "repeat-fib32", "f64-kernel"],
        )
        self.assertEqual(
            workloads[0]["sha256"],
            "e71e358234e39803a1d27961439d924a69c836dd81c8670bfff7dbb82c097bbe",
        )
        for workload in workloads[1:]:
            validation = workload["validation"]
            self.assertEqual(validation["expected_stdout"].format(n=7), "7\n")
            self.assertEqual(workload["export"], "run")
            source = baseline.REPO_ROOT / workload["source"]
            self.assertEqual(
                baseline.sha256_file(source), workload["source_sha256"]
            )
        self.assertEqual(
            {workload["name"]: workload["n"] for workload in workloads[1:]},
            {
                "loop-linear": 500_000_000,
                "repeat-fib32": 24,
                "f64-kernel": 500_000_000,
            },
        )

    def test_local_wat_workloads_compile_to_hashed_wasm(self):
        workloads = baseline.load_manifest(baseline.DEFAULT_MANIFEST)
        with tempfile.TemporaryDirectory() as temporary:
            args = SimpleNamespace(target_dir=Path(temporary), wasm_tools="wasm-tools")
            for workload in workloads[1:]:
                artifact = baseline.compile_wat_workload(
                    workload, args.target_dir, args.wasm_tools
                )
                output = Path(artifact["path"])
                self.assertTrue(output.is_file())
                self.assertEqual(artifact["sha256"], baseline.sha256_file(output))

    def test_materialization_records_one_failure_and_keeps_other_workloads_ready(self):
        workloads = baseline.load_manifest(baseline.DEFAULT_MANIFEST)
        bad = dict(workloads[2])
        bad["source_sha256"] = "0" * 64
        with tempfile.TemporaryDirectory() as temporary:
            args = SimpleNamespace(target_dir=Path(temporary), wasm_tools="wasm-tools")
            records = baseline.materialize_workloads([workloads[1], bad], args)

        self.assertEqual(records[0]["artifact_status"], "ready")
        self.assertEqual(records[1]["artifact_status"], "invalid")
        self.assertEqual(
            records[1]["invalid_reason"], "workload_source_hash_mismatch"
        )


class StatisticsContractTests(unittest.TestCase):
    def test_l2_schedule_includes_all_cells_and_scale_points(self):
        workload = baseline.load_manifest(baseline.DEFAULT_MANIFEST)[1]
        items = baseline.physical_schedule_items(workload)

        self.assertEqual(len(items), len(baseline.ARM_CONFIGS) * 3)
        self.assertEqual(items["opt-off@2n"], ("opt-off", 1_000_000_000))
        plan = baseline.l2_block_schedule(rounds=2, seed=184)
        schedule = plan["schedule"]
        self.assertEqual(sorted(schedule[0]), sorted(items))
        self.assertEqual(sorted(schedule[1]), sorted(items))
        self.assertEqual(
            [name for name, _, _ in baseline.COMPARISONS],
            [
                "measure_switches_control",
                "measure_switches_control_with_jit",
                "jit_feature_interpreter_tax",
                "optimizer_pipeline_upper_bound",
                "optimizer_pipeline_upper_bound_with_jit",
            ],
        )

    def test_quick_delta_never_becomes_a_quotable_number(self):
        quick = baseline.comparison_record(
            "candidate",
            [10.0, 10.0, 10.0],
            [20.0, 20.0, 20.0],
            floor=0.01,
            quick=True,
            seed=184,
        )
        self.assertEqual(quick["status"], "invalid")
        self.assertEqual(
            quick["invalid_reason"], "insufficient_samples_for_interval"
        )

        below = baseline.comparison_record(
            "candidate",
            [10.0, 10.0, 10.0],
            [10.0, 10.0, 10.0],
            floor=0.01,
            quick=True,
            seed=184,
        )
        self.assertEqual(below["status"], "below_noise_floor")
        self.assertNotIn("relative_delta", below)

    def test_published_delta_carries_a_bootstrap_interval(self):
        reported = baseline.comparison_record(
            "candidate",
            [10.0] * 10,
            [20.0] * 10,
            floor=0.01,
            quick=False,
            seed=184,
        )
        self.assertEqual(reported["status"], "reported_delta")
        self.assertIsNotNone(reported["bootstrap_percentile_ci_95"])
        self.assertIn("relative_delta", reported)


class L2WilliamsScheduleTests(unittest.TestCase):
    def test_blocks_keep_all_three_scales_adjacent(self):
        plan = baseline.l2_block_schedule(rounds=15, seed=184)

        for row in plan["schedule"]:
            self.assertEqual(len(row), len(baseline.ARM_CONFIGS) * 3)
            for offset in range(0, len(row), 3):
                block = row[offset : offset + 3]
                arms = {label.split("@", 1)[0] for label in block}
                scales = {label.split("@", 1)[1] for label in block}
                self.assertEqual(len(arms), 1)
                self.assertEqual(scales, {"1n", "2n", "3n"})

    def test_arm_blocks_have_exact_full_cycle_balance(self):
        plan = baseline.l2_block_schedule(rounds=14, seed=184)
        audit = plan["metadata"]["arm_block_order"]["balance_audit"]
        coremark = baseline.williams_schedule(
            list(baseline.ARM_CONFIGS), rounds=14, seed=184
        )

        self.assertEqual(
            [
                [label.split("@", 1)[0] for label in row[::3]]
                for row in plan["schedule"]
            ],
            coremark["schedule"],
        )

        for arm in baseline.ARM_CONFIGS:
            self.assertEqual(set(audit["positions"]["counts"][arm].values()), {2})
            self.assertEqual(
                set(audit["within_round_directed_carryover"]["counts"][arm].values()),
                {2},
            )

    def test_l2_metadata_exposes_cycle_and_boundary_audit_without_exact_claim(self):
        metadata = baseline.l2_block_schedule(rounds=15, seed=184)["metadata"]
        boundary = metadata["balance_audit"]["round_boundary_directed_carryover"]

        self.assertEqual(metadata["full_cycles"], 1)
        self.assertEqual(metadata["residual_rounds"], 1)
        self.assertEqual(metadata["carryover_scope"], "within_round")
        self.assertFalse(boundary["exact_balance_claimed"])
        self.assertEqual(boundary["imbalance"], boundary["max"] - boundary["min"])

    def test_scale_orders_record_balanced_prefix_and_honest_residual(self):
        plan = baseline.l2_block_schedule(rounds=15, seed=184)
        scale_order = plan["metadata"]["scale_order"]
        group = scale_order["groups"][0]

        self.assertEqual(set(group["balanced_prefix_permutation_counts"].values()), {2})
        self.assertEqual(group["residual_round_indices"], [12, 13, 14])
        self.assertEqual(len(group["residual_permutations"]), 3)
        self.assertFalse(scale_order["exact_permutation_balance_claimed"])
        self.assertEqual(
            sorted(scale_order["permutation_counts_per_round"].values()),
            [2, 2, 2, 3, 3, 3],
        )

    def test_twelve_scale_rounds_use_each_permutation_exactly_twice(self):
        plan = baseline.l2_scale_permutation_plan(rounds=12, seed=184)
        metadata = plan["metadata"]

        self.assertEqual(set(metadata["permutation_counts_per_round"].values()), {2})
        self.assertEqual(metadata["groups"][0]["balanced_prefix_rounds"], 12)
        self.assertTrue(metadata["groups"][0]["balanced_prefix_complete"])
        self.assertEqual(metadata["groups"][0]["residual_round_indices"], [])

    def test_fifteen_round_scale_positions_are_exactly_five_per_arm(self):
        plan = baseline.l2_block_schedule(rounds=15, seed=184)
        position_balance = plan["metadata"]["scale_order"]["position_balance"]

        self.assertTrue(position_balance["per_arm_exactly_balanced"])
        for arm in baseline.ARM_CONFIGS:
            for scale in ("1n", "2n", "3n"):
                self.assertEqual(
                    set(position_balance["per_arm_counts"][arm][scale].values()), {5}
                )

    def test_l2_schedule_is_deterministic_for_normal_and_quick_round_counts(self):
        self.assertEqual(
            baseline.l2_block_schedule(rounds=15, seed=184),
            baseline.l2_block_schedule(rounds=15, seed=184),
        )
        quick = baseline.l2_block_schedule(rounds=3, seed=184)
        metadata = quick["metadata"]["scale_order"]
        self.assertEqual(quick, baseline.l2_block_schedule(rounds=3, seed=184))
        self.assertEqual(metadata["full_fifteen_round_groups"], 0)
        self.assertEqual(metadata["partial_group_rounds"], 3)
        self.assertFalse(metadata["exact_permutation_balance_claimed"])
        self.assertFalse(metadata["groups"][0]["balanced_prefix_complete"])
        self.assertEqual(
            sum(metadata["groups"][0]["balanced_prefix_permutation_counts"].values()),
            3,
        )

    def test_l1_workload_schedule_exposes_williams_metadata(self):
        workload = baseline.load_manifest(baseline.DEFAULT_MANIFEST)[0]
        plan = baseline.workload_schedule(workload, rounds=15, seed=184)
        metadata = plan["metadata"]

        self.assertEqual(metadata["method"], "williams_carryover_balanced_latin_square")
        self.assertEqual(metadata["carryover_scope"], "within_round")
        self.assertEqual(metadata["full_cycles"], 1)
        self.assertEqual(metadata["residual_rounds"], 1)


class SampleLoadTests(unittest.TestCase):
    def test_observed_sample_load_returns_finite_one_minute_value(self):
        with mock.patch.object(baseline.os, "getloadavg", return_value=(1.25, 0, 0)):
            self.assertEqual(baseline.observed_sample_load_average(), 1.25)

    def test_observed_sample_load_fails_closed_when_unavailable(self):
        with mock.patch.object(baseline.os, "getloadavg", side_effect=OSError("no load")):
            with self.assertRaises(baseline.BaselineError) as unavailable:
                baseline.observed_sample_load_average()
        self.assertEqual(unavailable.exception.reason, "sample_load_unavailable")

    def test_observed_sample_load_rejects_missing_load_tuple(self):
        with mock.patch.object(baseline.os, "getloadavg", return_value=None):
            with self.assertRaises(baseline.BaselineError) as unavailable:
                baseline.observed_sample_load_average()
        self.assertEqual(unavailable.exception.reason, "sample_load_unavailable")

    def test_observed_sample_load_fails_closed_when_non_finite(self):
        with mock.patch.object(
            baseline.os, "getloadavg", return_value=(float("nan"), 0, 0)
        ):
            with self.assertRaises(baseline.BaselineError) as non_finite:
                baseline.observed_sample_load_average()
        self.assertEqual(non_finite.exception.reason, "sample_load_non_finite")

    def test_sample_record_contains_load_before_and_after(self):
        workload = {
            "name": "synthetic",
            "kind": "wat",
            "artifact": {"path": "/private/tmp/synthetic.wasm"},
            "export": "run",
            "validation": {"kind": "exact_stdout", "expected_stdout": "{n}\n"},
        }
        with mock.patch.object(
            baseline.os, "getloadavg", side_effect=[(1.0, 0, 0), (1.5, 0, 0)]
        ), mock.patch.object(baseline, "checked_output", return_value="7\n"), mock.patch.object(
            baseline.time, "perf_counter_ns", side_effect=[100, 250]
        ):
            sample = baseline.run_one_sample(
                Path("/private/tmp/telomere-cli"), workload, None, 7
            )

        self.assertEqual(sample["wall_ns"], 150)
        self.assertEqual(sample["load_average_1m_before"], 1.0)
        self.assertEqual(sample["load_average_1m_after"], 1.5)


class CompareWitnessContractTests(unittest.TestCase):
    def test_comparison_extracts_a_witness_for_each_matching_build(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "prior.json"
            builds = {
                name: {"features": list(config["features"])}
                for name, config in baseline.BUILD_CONFIGS.items()
            }
            prior = {
                "status": "ok",
                "publishable": True,
                "matrix": {"builds": builds},
                "tail_call_witnesses": {
                    name: {"probes": [], "contract_passed": True}
                    for name in baseline.BUILD_CONFIGS
                },
            }
            path.write_text(json.dumps(prior), encoding="utf-8")

            selected = baseline.load_prior_build_witnesses(path, builds)

            self.assertEqual(set(selected), set(baseline.BUILD_CONFIGS))

    def test_comparison_rejects_non_publishable_prior_raw_record(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "prior.json"
            builds = {
                name: {"features": list(config["features"])}
                for name, config in baseline.BUILD_CONFIGS.items()
            }
            for status, publishable in (("invalid", True), ("ok", False)):
                path.write_text(
                    json.dumps(
                        {
                            "status": status,
                            "publishable": publishable,
                            "matrix": {"builds": builds},
                            "tail_call_witnesses": {},
                        }
                    ),
                    encoding="utf-8",
                )
                with self.assertRaises(baseline.BaselineError) as rejected:
                    baseline.load_prior_build_witnesses(path, builds)
                self.assertEqual(rejected.exception.reason, "baseline_not_publishable")


class CompareArtifactContractTests(unittest.TestCase):
    def test_comparison_requires_an_identical_artifact_identity_set(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "prior.json"
            prior = {
                "status": "ok",
                "publishable": True,
                "artifacts": [
                    {
                        "name": "l2-loop",
                        "layer": "L2",
                        "artifact": {"sha256": "a" * 64},
                    },
                    {
                        "name": "l1-coremark",
                        "layer": "L1",
                        "artifact": {"sha256": "b" * 64},
                    },
                ],
            }
            path.write_text(json.dumps(prior), encoding="utf-8")
            current = [
                {
                    "name": "l1-coremark",
                    "layer": "L1",
                    "artifact_status": "ready",
                    "artifact": {"sha256": "b" * 64},
                },
                {
                    "name": "l2-loop",
                    "layer": "L2",
                    "artifact_status": "ready",
                    "artifact": {"sha256": "a" * 64},
                },
            ]

            result = baseline.validate_compare_artifacts(path, current)

            self.assertEqual(result["status"], "pass")
            self.assertEqual(len(result["identities"]), 2)

            current[1]["artifact"] = {"sha256": "c" * 64}
            with self.assertRaises(baseline.BaselineError) as mismatch:
                baseline.validate_compare_artifacts(path, current)
            self.assertEqual(mismatch.exception.reason, "baseline_artifact_mismatch")


if __name__ == "__main__":
    unittest.main()
