#!/usr/bin/env python3
"""Unit tests for the fn-pointer identity checker."""

from __future__ import annotations

import importlib.util
import sys
import unittest
from pathlib import Path


TOOLS_DIR = Path(__file__).resolve().parent
MODULE_PATH = TOOLS_DIR / "check-fn-identity.py"
SPEC = importlib.util.spec_from_file_location("check_fn_identity", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
fn_identity = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = fn_identity
SPEC.loader.exec_module(fn_identity)


PLAIN = "crates/telomere/src/runtime/vm/control.rs"
COMPARE = "crates/telomere/src/runtime/jit/backend/ops.rs"
SIMD = "crates/telomere/src/runtime/vm/simd.rs"
MACROS = "crates/telomere-macros/src/lib.rs"


def handler_def(protected: bool) -> str:
    attr = "#[inline(never)]\n" if protected else ""
    return (
        "/// Docs.\n"
        f"{attr}"
        "pub unsafe fn op_example(tail_code: *const Instr, ctx: &mut ExecuteContext) "
        "-> VMResult<()> {\n    VMResult::Success(())\n}\n"
    )


def compare_site() -> str:
    return (
        "fn decode(op: Op) -> bool {\n"
        "    std::ptr::fn_addr_eq(op, op_example as Op)\n"
        "}\n"
    )


class PlainDefinitionTests(unittest.TestCase):
    def test_protected_compared_handler_passes(self) -> None:
        sources = {PLAIN: handler_def(True), COMPARE: compare_site()}
        self.assertEqual(fn_identity.find_violations(sources), [])

    def test_unprotected_compared_handler_fails(self) -> None:
        sources = {PLAIN: handler_def(False), COMPARE: compare_site()}
        violations = fn_identity.find_violations(sources)
        self.assertEqual(len(violations), 1)
        self.assertIn("op_example", violations[0])
        self.assertIn("#[inline(never)]", violations[0])

    def test_uncompared_handler_is_not_checked(self) -> None:
        sources = {PLAIN: handler_def(False)}
        self.assertEqual(fn_identity.find_violations(sources), [])

    def test_macro_variable_is_not_a_compared_handler(self) -> None:
        sources = {
            PLAIN: handler_def(False),
            COMPARE: "if std::ptr::fn_addr_eq(op, $handler as Op) { return; }\n",
        }
        self.assertEqual(fn_identity.find_violations(sources), [])

    def test_op_eq_comparison_is_detected(self) -> None:
        sources = {
            PLAIN: handler_def(False),
            COMPARE: "fn f(i: &I) -> bool { i.op_eq(op_example as Op) }\n",
        }
        violations = fn_identity.find_violations(sources)
        self.assertEqual(len(violations), 1)
        self.assertIn("op_example", violations[0])

    def test_unresolvable_compared_handler_fails(self) -> None:
        sources = {COMPARE: compare_site()}
        violations = fn_identity.find_violations(sources)
        self.assertEqual(len(violations), 1)
        self.assertIn("could not be resolved", violations[0])


class AliasTests(unittest.TestCase):
    def test_alias_resolves_to_target_protection(self) -> None:
        sources = {
            PLAIN: handler_def(True)
            + "pub(crate) use op_example as op_example_alias;\n",
            COMPARE: (
                "fn decode(op: Op) -> bool {\n"
                "    std::ptr::fn_addr_eq(op, op_example_alias as Op)\n"
                "}\n"
            ),
        }
        self.assertEqual(fn_identity.find_violations(sources), [])

    def test_alias_to_unprotected_target_fails(self) -> None:
        sources = {
            PLAIN: handler_def(False)
            + "pub(crate) use op_example as op_example_alias;\n",
            COMPARE: (
                "fn decode(op: Op) -> bool {\n"
                "    std::ptr::fn_addr_eq(op, op_example_alias as Op)\n"
                "}\n"
            ),
        }
        violations = fn_identity.find_violations(sources)
        self.assertEqual(len(violations), 1)
        self.assertIn("op_example_alias", violations[0])


TEMPLATE = """macro_rules! define_example {
    ($name:ident) => {
        #[doc = "Docs."]
        ATTRPub unsafe fn $name(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
            VMResult::Success(())
        }
    };
}
define_example!(op_example);
"""


class MacroTemplateTests(unittest.TestCase):
    def test_protected_template_product_passes(self) -> None:
        sources = {
            PLAIN: TEMPLATE.replace("ATTR", "#[inline(never)]\n        "),
            COMPARE: compare_site(),
        }
        self.assertEqual(fn_identity.find_violations(sources), [])

    def test_unprotected_template_product_fails(self) -> None:
        sources = {
            PLAIN: TEMPLATE.replace("ATTR", ""),
            COMPARE: compare_site(),
        }
        violations = fn_identity.find_violations(sources)
        self.assertEqual(len(violations), 1)
        self.assertIn("op_example", violations[0])
        self.assertIn("macro define_example", violations[0])


SIMD_INVOKE = "define_unary_simd_operation!(swizzle, [i8x16], |a, b| a.swizzle(b));\n"


def macros_lib(protected: bool) -> str:
    attr = "            #[inline(never)]\n" if protected else ""
    return (
        "fn generate() {\n"
        "    stream.extend(quote! {\n"
        f"{attr}"
        "            pub unsafe fn #mangled(tail_code: *const Instr) {}\n"
        "    });\n"
        "}\n"
    )


class SimdProcMacroTests(unittest.TestCase):
    def test_protected_proc_macro_product_passes(self) -> None:
        sources = {
            SIMD: SIMD_INVOKE,
            MACROS: macros_lib(True),
            COMPARE: (
                "fn decode(op: Op) -> bool {\n"
                "    std::ptr::fn_addr_eq(op, i8x16_swizzle as Op)\n"
                "}\n"
            ),
        }
        self.assertEqual(fn_identity.find_violations(sources), [])

    def test_unprotected_proc_macro_product_fails(self) -> None:
        sources = {
            SIMD: SIMD_INVOKE,
            MACROS: macros_lib(False),
            COMPARE: (
                "fn decode(op: Op) -> bool {\n"
                "    std::ptr::fn_addr_eq(op, i8x16_swizzle as Op)\n"
                "}\n"
            ),
        }
        violations = fn_identity.find_violations(sources)
        self.assertEqual(len(violations), 1)
        self.assertIn("i8x16_swizzle", violations[0])


class OpSpecsTests(unittest.TestCase):
    def test_op_specs_entry_is_detected(self) -> None:
        sources = {
            PLAIN: handler_def(False),
            COMPARE: (
                "const OP_SPECS: &[OpSpec] = &[\n"
                "    OpSpec { op: op_example as Op, decode: decode_example },\n"
                "];\n"
            ),
        }
        violations = fn_identity.find_violations(sources)
        self.assertEqual(len(violations), 1)
        self.assertIn("op_example", violations[0])


if __name__ == "__main__":
    unittest.main()
