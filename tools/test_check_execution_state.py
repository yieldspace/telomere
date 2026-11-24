#!/usr/bin/env python3
"""Unit tests for the execution-state placement checker."""

from __future__ import annotations

import importlib.util
import sys
import unittest
from pathlib import Path


TOOLS_DIR = Path(__file__).resolve().parent
MODULE_PATH = TOOLS_DIR / "check-execution-state.py"
SPEC = importlib.util.spec_from_file_location("check_execution_state", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
execution_state = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = execution_state
SPEC.loader.exec_module(execution_state)


VM = "crates/telomere/src/runtime/vm.rs"
JIT = "crates/telomere/src/runtime/jit.rs"
COMMON = "crates/telomere/src/common.rs"


class ThreadLocalStaticsTest(unittest.TestCase):
    def test_finds_single_static(self):
        text = "thread_local! {\n    static FOO: Cell<u64> = const { Cell::new(0) };\n}\n"
        self.assertEqual(execution_state.thread_local_statics(text), ["FOO"])

    def test_finds_multiple_statics_in_one_block(self):
        text = (
            "thread_local! {\n"
            "    static FIRST: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };\n"
            "    static SECOND: Cell<bool> = const { Cell::new(false) };\n"
            "}\n"
        )
        self.assertEqual(
            execution_state.thread_local_statics(text), ["FIRST", "SECOND"]
        )

    def test_ignores_statics_outside_thread_local(self):
        text = "static PLAIN: AtomicU64 = AtomicU64::new(0);\n"
        self.assertEqual(execution_state.thread_local_statics(text), [])

    def test_nested_braces_do_not_end_block_early(self):
        text = (
            "thread_local! {\n"
            "    static FOO: RefCell<Option<S>> = const { RefCell::new(None) };\n"
            "}\n"
            "static AFTER: u32 = 0;\n"
        )
        self.assertEqual(execution_state.thread_local_statics(text), ["FOO"])


class CheckSourceTest(unittest.TestCase):
    def test_clean_file_passes(self):
        self.assertEqual(execution_state.check_source(JIT, "fn dispatch() {}\n"), [])

    def test_new_thread_local_is_a_violation(self):
        text = "thread_local! {\n    static NEW_SLOT: Cell<u64> = const { Cell::new(0) };\n}\n"
        violations = execution_state.check_source(COMMON, text)
        self.assertEqual(len(violations), 1)
        self.assertIn("NEW_SLOT", violations[0])

    def test_allowlisted_profile_session_passes(self):
        text = (
            "#[cfg(feature = \"vm-profile\")]\n"
            "thread_local! {\n"
            "    static DISPATCH_PROFILE_SESSION: RefCell<Option<S>> = const { RefCell::new(None) };\n"
            "}\n"
        )
        self.assertEqual(execution_state.check_source(VM, text), [])

    def test_allowlist_is_per_file(self):
        text = "thread_local! {\n    static DISPATCH_PROFILE_SESSION: Cell<u64> = const { Cell::new(0) };\n}\n"
        violations = execution_state.check_source(JIT, text)
        self.assertEqual(len(violations), 1)


if __name__ == "__main__":
    unittest.main()
