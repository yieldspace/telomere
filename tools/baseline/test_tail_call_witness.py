#!/usr/bin/env python3
"""Regression tests for the path-aware tail-call witness parser."""

from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock


HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

from tail_call_witness import (  # noqa: E402
    PROBES,
    analyse_disassembly,
    capture_tail_call_witness,
)


FIXTURES = HERE / "fixtures"


def witness_from_fixture(name: str, architecture: str = "arm64") -> dict:
    return analyse_disassembly(
        (FIXTURES / name).read_text(encoding="utf-8"), architecture
    )


def capture_fixture_comparison(current_disassembly: str, prior_witness: dict) -> dict:
    """Exercise the public Mapping comparison API without a host toolchain."""

    with tempfile.NamedTemporaryFile() as binary:
        binary.write(b"fixture binary")
        binary.flush()
        with mock.patch(
            "tail_call_witness.detect_binary_architecture", return_value="arm64"
        ), mock.patch(
            "tail_call_witness.disassemble_binary",
            return_value=(current_disassembly, ["fixture-disassembler"]),
        ):
            return capture_tail_call_witness(
                Path(binary.name),
                mode="compare",
                baseline_witness=prior_witness,
            )


class TailCallWitnessParserTests(unittest.TestCase):
    def test_arm64_healthy_fixture_verifies_every_probe(self) -> None:
        witness = witness_from_fixture("arm64-healthy.s")

        self.assertTrue(witness["contract_passed"])
        self.assertEqual(witness["probe_coverage"], "4 of 4 probes verified")
        self.assertEqual([probe["probe"] for probe in witness["probes"]], list(PROBES))
        self.assertEqual(
            [len(probe["dispatch_exits"]) for probe in witness["probes"]],
            [1, 2, 1, 1],
        )

    def test_x86_64_healthy_fixture_accepts_indirect_jumps(self) -> None:
        witness = witness_from_fixture("x86_64-healthy.s", "x86_64")

        self.assertTrue(witness["contract_passed"])
        self.assertEqual(witness["probe_coverage"], "4 of 4 probes verified")
        for probe in witness["probes"]:
            self.assertTrue(
                all(exit_["kind"] == "tail_branch" for exit_ in probe["dispatch_exits"])
            )

    def test_x86_64_returning_helper_before_tail_dispatch_is_excluded(self) -> None:
        witness = witness_from_fixture(
            "x86_64-returning-helper-before-tail-dispatch.s", "x86_64"
        )
        call_probe = next(probe for probe in witness["probes"] if probe["probe"] == "op_call")

        self.assertTrue(witness["contract_passed"])
        self.assertEqual([exit_["kind"] for exit_ in call_probe["dispatch_exits"]], ["tail_branch"])
        self.assertEqual(len(call_probe["excluded_dispatch_transfers"]), 1)
        helper = call_probe["excluded_dispatch_transfers"][0]
        self.assertEqual(
            helper["reason"], "returning_indirect_helper_before_tail_dispatch"
        )
        self.assertTrue(helper["ret_reachable_after_transfer"])
        self.assertIn("jmpq", helper["tail_dispatch_instruction"])

    def test_call_and_ret_degradation_fails_the_absolute_contract(self) -> None:
        witness = witness_from_fixture("arm64-degraded-call-ret.s")
        call_probe = next(probe for probe in witness["probes"] if probe["probe"] == "op_call")

        self.assertFalse(witness["contract_passed"])
        self.assertEqual(witness["status"], "fail")
        self.assertEqual(call_probe["status"], "fail")
        self.assertEqual(call_probe["dispatch_exits"][0]["kind"], "indirect_call")
        self.assertTrue(call_probe["dispatch_exits"][0]["ret_reachable_after_transfer"])
        self.assertEqual(call_probe["excluded_dispatch_transfers"], [])

    def test_returning_helper_before_tail_dispatch_is_excluded_with_evidence(self) -> None:
        witness = witness_from_fixture("arm64-returning-helper-before-tail-dispatch.s")

        self.assertTrue(witness["contract_passed"])
        self.assertEqual(witness["probe_coverage"], "4 of 4 probes verified")
        for probe in witness["probes"]:
            self.assertEqual(
                [exit_["kind"] for exit_ in probe["dispatch_exits"]], ["tail_branch"]
            )
            self.assertEqual(len(probe["excluded_dispatch_transfers"]), 1)
            helper = probe["excluded_dispatch_transfers"][0]
            self.assertEqual(
                helper["reason"], "returning_indirect_helper_before_tail_dispatch"
            )
            self.assertTrue(helper["ret_reachable_after_transfer"])
            self.assertEqual(
                helper["tail_dispatch_address"], probe["dispatch_exits"][0]["address"]
            )
            self.assertIn("br\tx2", helper["tail_dispatch_instruction"])

    def test_physical_tail_cold_panic_is_excluded_without_failing(self) -> None:
        witness = witness_from_fixture("arm64-cold-panic-at-physical-end.s")
        numeric_probe = next(
            probe for probe in witness["probes"] if probe["probe"] == "op_i32_and"
        )

        self.assertTrue(witness["contract_passed"])
        self.assertTrue(
            any(
                exclusion["reason"] == "known_panic_symbol"
                and not exclusion["reachable_from_entry"]
                for exclusion in numeric_probe["excluded_blocks"]
            )
        )

    def test_all_reachable_normal_exits_are_required(self) -> None:
        witness = witness_from_fixture("arm64-multi-success-partially-degraded.s")
        branch_probe = next(
            probe for probe in witness["probes"] if probe["probe"] == "op_local_get4_br_if"
        )

        self.assertFalse(witness["contract_passed"])
        self.assertEqual(branch_probe["status"], "fail")
        self.assertEqual(len(branch_probe["dispatch_exits"]), 2)
        self.assertEqual(
            {exit_["kind"] for exit_ in branch_probe["dispatch_exits"]},
            {"tail_branch", "indirect_call"},
        )
        self.assertEqual(branch_probe["excluded_dispatch_transfers"], [])

    def test_unknown_architecture_fails_closed(self) -> None:
        witness = witness_from_fixture("arm64-healthy.s", "riscv64")

        self.assertFalse(witness["contract_passed"])
        self.assertEqual(witness["status"], "witness_unavailable")
        self.assertEqual(witness["invalid_reason"], "unsupported_architecture")

    def test_compare_uses_the_explicit_prior_witness_mapping(self) -> None:
        healthy = (FIXTURES / "arm64-healthy.s").read_text(encoding="utf-8")
        prior = analyse_disassembly(healthy, "arm64")

        witness = capture_fixture_comparison(healthy, prior)

        self.assertTrue(witness["absolute_contract_passed"])
        self.assertTrue(witness["contract_passed"])
        self.assertEqual(witness["relative_comparison"], {"status": "pass", "regressions": []})

    def test_compare_rejects_an_invalid_prior_witness_mapping(self) -> None:
        healthy = (FIXTURES / "arm64-healthy.s").read_text(encoding="utf-8")

        witness = capture_fixture_comparison(healthy, {})

        self.assertTrue(witness["absolute_contract_passed"])
        self.assertFalse(witness["contract_passed"])
        self.assertEqual(witness["status"], "fail")
        self.assertEqual(witness["invalid_reason"], "baseline_witness_invalid")
        self.assertEqual(
            witness["relative_comparison"],
            {"status": "fail", "regressions": ["baseline_witness_invalid"]},
        )

    def test_compare_rejects_a_prior_exit_count_that_is_not_observed_now(self) -> None:
        healthy = (FIXTURES / "arm64-healthy.s").read_text(encoding="utf-8")
        prior = analyse_disassembly(healthy, "arm64")
        one_branch_exit = healthy.replace(
            "0000000100001010\tcbz\tw8, 0x100001020\n"
            "0000000100001014\tldr\tx2, [x0], #0x8\n"
            "0000000100001018\tbr\tx2\n"
            "0000000100001020\tldr\tx2, [x0], #0x8\n"
            "0000000100001024\tbr\tx2\n",
            "0000000100001010\tldr\tx2, [x0], #0x8\n"
            "0000000100001014\tbr\tx2\n",
        )

        witness = capture_fixture_comparison(one_branch_exit, prior)

        self.assertTrue(witness["absolute_contract_passed"])
        self.assertFalse(witness["contract_passed"])
        self.assertEqual(witness["status"], "fail")
        self.assertEqual(witness["invalid_reason"], "relative_witness_regression")
        self.assertEqual(
            witness["relative_comparison"],
            {
                "status": "fail",
                "regressions": ["dispatch_exit_count_regression:op_local_get4_br_if"],
            },
        )

    def test_compare_keeps_the_absolute_contract_when_relative_coverage_matches(self) -> None:
        healthy = (FIXTURES / "arm64-healthy.s").read_text(encoding="utf-8")
        degraded = healthy.replace(
            "0000000100001040\tldr\tx2, [x0], #0x8\n"
            "0000000100001044\tbr\tx2\n",
            "0000000100001040\tldr\tx2, [x0], #0x8\n"
            "0000000100001044\tblr\tx2\n"
            "0000000100001048\tret\n",
        )
        prior = analyse_disassembly(healthy, "arm64")

        witness = capture_fixture_comparison(degraded, prior)

        self.assertFalse(witness["absolute_contract_passed"])
        self.assertFalse(witness["contract_passed"])
        self.assertEqual(witness["status"], "fail")
        self.assertEqual(witness["invalid_reason"], "absolute_tail_call_contract_failed")
        self.assertEqual(witness["relative_comparison"], {"status": "pass", "regressions": []})

    def test_compare_rejects_two_baseline_sources(self) -> None:
        with self.assertRaisesRegex(ValueError, "exactly one baseline source"):
            capture_tail_call_witness(
                "not-needed-for-argument-validation",
                mode="compare",
                baseline_path="prior.json",
                baseline_witness={},
            )


if __name__ == "__main__":
    unittest.main()
