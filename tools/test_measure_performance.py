#!/usr/bin/env python3
"""Unit tests for the performance gate measure script's pure helpers."""

from __future__ import annotations

import importlib.util
import sys
import unittest
from pathlib import Path


TOOLS_DIR = Path(__file__).resolve().parent
MODULE_PATH = TOOLS_DIR / "measure-performance.py"
SPEC = importlib.util.spec_from_file_location("measure_performance", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
measure_perf = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = measure_perf
SPEC.loader.exec_module(measure_perf)


class CellCommandTests(unittest.TestCase):
    def test_jit_cell_passes_jit_flag_before_artifact(self) -> None:
        command = measure_perf.cell_command(
            "jit-jit", Path("/bin/cli"), Path("/w.wasm"), "run", 24
        )
        self.assertEqual(command, ["/bin/cli", "--jit", "/w.wasm", "run", "24"])

    def test_interpreter_cells_have_no_jit_flag(self) -> None:
        for cell in ("default-interp", "jit-interp"):
            command = measure_perf.cell_command(
                cell, Path("/bin/cli"), Path("/w.wasm"), "run", 24
            )
            self.assertEqual(command, ["/bin/cli", "/w.wasm", "run", "24"])


class SummarizeTests(unittest.TestCase):
    def test_median_min_max_and_count(self) -> None:
        samples = [
            {"wall_ns": value, "load_average_1m_before": 0.1, "load_average_1m_after": 0.1}
            for value in (30, 10, 20, 50, 40)
        ]
        summary = measure_perf.summarize(samples)
        self.assertEqual(summary["median_ns"], 30)
        self.assertEqual(summary["min_ns"], 10)
        self.assertEqual(summary["max_ns"], 50)
        self.assertEqual(summary["sample_count"], 5)


class ManifestTests(unittest.TestCase):
    def test_real_manifest_yields_only_loop_linear_and_repeat_fib32(self) -> None:
        workloads = measure_perf.load_workloads(measure_perf.DEFAULT_MANIFEST)
        names = [workload["name"] for workload in workloads]
        # coremark is remote (its pinned digest no longer matches the
        # served artifact) and f64-kernel aborts in the interpreter at
        # scale; both are excluded with evidence in
        # docs/benchmarks/performance.md.
        self.assertEqual(names, ["loop-linear", "repeat-fib32"])

    def test_expected_stdout_uses_manifest_template(self) -> None:
        workloads = measure_perf.load_workloads(measure_perf.DEFAULT_MANIFEST)
        for workload in workloads:
            self.assertEqual(
                measure_perf.expected_stdout(workload, workload["n"]),
                f"{workload['n']}\n",
            )


if __name__ == "__main__":
    unittest.main()
