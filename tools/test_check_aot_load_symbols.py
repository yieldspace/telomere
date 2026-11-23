#!/usr/bin/env python3
"""Unit tests for tools/check-aot-load-symbols.py (stdlib only)."""

import importlib.util
import sys
import unittest
from pathlib import Path


TOOLS_DIR = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location(
    "check_aot_load_symbols", TOOLS_DIR / "check-aot-load-symbols.py"
)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


CLEAN_SYMBOLS = [
    "_ZN8telomere7runtime3jit5cache13StoreJitCache10get_cached17h1234abcdE",
    "_ZN17telomere_jit_codegen11code_memory9CodeArena8allocate17h1234abcdE",
    "_ZN8telomere7runtime11instantiate11instantiate17h1234abcdE",
    "_ZN8telomere7runtime3jit5stubs22runtime_stub_data_drop17h1234abcdE",
    "main",
]


class FindViolationsTests(unittest.TestCase):
    def test_clean_symbols_pass(self):
        self.assertEqual(MODULE.find_violations(CLEAN_SYMBOLS), [])

    def test_emitter_function_symbol_fails(self):
        symbols = CLEAN_SYMBOLS + [
            "_ZN8telomere7runtime3jit7backend22emit_baseline_function17h1234abcdE"
        ]
        failures = MODULE.find_violations(symbols)
        self.assertEqual(len(failures), 1)
        self.assertIn("emit_baseline_function", failures[0])

    def test_plan_symbols_fail(self):
        for leaked in ("emit_baseline_plan", "build_baseline_plan"):
            symbols = CLEAN_SYMBOLS + [f"_ZN8telomere7runtime3jit7backend4ops18{leaked}17hE"]
            failures = MODULE.find_violations(symbols)
            self.assertTrue(
                any(leaked in failure for failure in failures),
                f"{leaked} must be reported, got {failures}",
            )

    def test_missing_load_path_symbol_fails(self):
        symbols = [name for name in CLEAN_SYMBOLS if "CodeArena" not in name]
        failures = MODULE.find_violations(symbols)
        self.assertTrue(any("CodeArena" in failure for failure in failures))

    def test_empty_symbol_list_fails(self):
        failures = MODULE.find_violations([])
        self.assertEqual(len(failures), len(MODULE.REQUIRED_SYMBOLS))


class ParseNmOutputTests(unittest.TestCase):
    def test_defined_and_undefined_lines(self):
        text = (
            "\n"
            "/path/to/binary:\n"
            "0000000000123456 T _ZN8telomere7runtime11instantiate11instantiate17hE\n"
            "                 U __libc_start_main\n"
            "0000000000123457 t local_symbol\n"
        )
        names = MODULE.parse_nm_output(text)
        self.assertIn(
            "_ZN8telomere7runtime11instantiate11instantiate17hE", names
        )
        self.assertIn("__libc_start_main", names)
        self.assertIn("local_symbol", names)

    def test_blank_output(self):
        self.assertEqual(MODULE.parse_nm_output(""), [])


if __name__ == "__main__":
    unittest.main()
