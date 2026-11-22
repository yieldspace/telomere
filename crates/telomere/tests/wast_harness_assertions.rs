//! Regression tests for the wast harness's own spec fidelity (#131).
//!
//! Each `REJECTED` case states a spec expectation that does not hold for the
//! module or execution; the harness must fail it instead of silently
//! accepting "some error occurred". Each `ACCEPTED` case states the correct
//! expectation and must pass. Before #131 every rejected case below was
//! accepted by the harness.
//!
//! The v128 lane cases live in the `*_SIMD` constants and test functions,
//! gated on the `simd` feature: without it the parser rejects v128 modules
//! outright, so those cases cannot exercise the harness's NaN-pattern
//! matching at all. With `simd` enabled (the default) every case runs.

mod common;

use std::panic::{catch_unwind, AssertUnwindSafe};

fn run_wast_outcome(text: &str) -> Result<(), ()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let text = text.to_owned();
    catch_unwind(AssertUnwindSafe(move || {
        runtime.block_on(async move { common::run_wast(&text).await });
    }))
    .map_err(|_| ())
}

/// The trap reason differs from the spec message (divide by zero is not
/// "unreachable").
const WRONG_TRAP_REASON: &str = r#"
(module (func (export "f") (result i32) i32.const 1 i32.const 0 i32.div_s))
(assert_trap (invoke "f") "unreachable")
"#;

/// The function returns two values but only one is expected.
const ARITY_TOO_FEW_EXPECTED: &str = r#"
(module (func (export "g") (result i32 i32) i32.const 1 i32.const 2))
(assert_return (invoke "g") (i32.const 1))
"#;

/// The function returns one value but two are expected.
const ARITY_TOO_MANY_EXPECTED: &str = r#"
(module (func (export "h") (result i32) i32.const 1))
(assert_return (invoke "h") (i32.const 1) (i32.const 2))
"#;

/// An arithmetic NaN with payload bits set is not the canonical NaN.
const NON_CANONICAL_AS_CANONICAL: &str = r#"
(module (func (export "n") (result f32) f32.const nan:0x7fffff))
(assert_return (invoke "n") (f32.const nan:canonical))
"#;

/// Same distinction in a v128 lane.
#[cfg(feature = "simd")]
const LANE_NON_CANONICAL_AS_CANONICAL: &str = r#"
(module (func (export "n") (result v128) (v128.const f32x4 nan:0x7fffff 0 0 0)))
(assert_return (invoke "n") (v128.const f32x4 nan:canonical 0 0 0))
"#;

/// The module is invalid because of an unknown function, not a type
/// mismatch.
const WRONG_INVALID_REASON: &str = r#"
(assert_invalid (module (func (call 7))) "type mismatch")
"#;

/// The module is valid (align=4 is the natural alignment of i32.load), so
/// the assertion itself must fail; the harness must not skip alignment
/// assertions.
const ALIGNMENT_ASSERTION_ON_VALID_MODULE: &str = r#"
(assert_invalid
  (module (memory 1) (func (result i32) i32.const 0 i32.load align=4))
  "alignment must not be larger than natural")
"#;

/// With neither a table nor an element segment, table.init validation
/// reports the unknown table first; claiming an unknown element segment is
/// the wrong reason.
const TABLE_INIT_WRONG_REASON: &str = r#"
(assert_invalid
  (module (func (table.init 0 (i32.const 12) (i32.const 1) (i32.const 1))))
  "unknown elem segment 0")
"#;

/// An out-of-range branch is an unknown label, not a type mismatch.
const UNKNOWN_LABEL_AS_TYPE_MISMATCH: &str = r#"
(assert_invalid (module (func (br 1))) "type mismatch")
"#;

const REJECTED: &[(&str, &str)] = &[
    ("wrong trap reason", WRONG_TRAP_REASON),
    ("arity: too few expected", ARITY_TOO_FEW_EXPECTED),
    ("arity: too many expected", ARITY_TOO_MANY_EXPECTED),
    ("non-canonical NaN as canonical", NON_CANONICAL_AS_CANONICAL),
    ("wrong invalid reason", WRONG_INVALID_REASON),
    (
        "alignment assertion on a valid module",
        ALIGNMENT_ASSERTION_ON_VALID_MODULE,
    ),
    ("table.init wrong reason", TABLE_INIT_WRONG_REASON),
    (
        "unknown label as type mismatch",
        UNKNOWN_LABEL_AS_TYPE_MISMATCH,
    ),
];

/// Rejected expectations that need the `simd` feature to parse at all.
#[cfg(feature = "simd")]
const REJECTED_SIMD: &[(&str, &str)] = &[(
    "lane non-canonical NaN as canonical",
    LANE_NON_CANONICAL_AS_CANONICAL,
)];

const CORRECT_EXPECTATIONS: &str = r#"
(module
  (func (export "f") (result i32) i32.const 1 i32.const 0 i32.div_s)
  (func (export "g") (result i32 i32) i32.const 1 i32.const 2)
  (func (export "n") (result f32) f32.const nan)
  (func (export "a") (result f32) f32.const nan:0x7fffff)
)
(assert_trap (invoke "f") "integer divide by zero")
(assert_return (invoke "g") (i32.const 1) (i32.const 2))
(assert_return (invoke "n") (f32.const nan:canonical))
(assert_return (invoke "a") (f32.const nan:arithmetic))
(assert_invalid (module (func (call 7))) "unknown function")
(assert_invalid (module (func (br 1))) "unknown label")
(assert_invalid (module (export "a" (global 0))) "unknown global")
(assert_invalid
  (module (memory 1) (func (result i32) i32.const 0 i32.load align=8))
  "alignment must not be larger than natural")
(assert_invalid
  (module (memory 1) (func (result i64) i64.const 0 i64.load align=16))
  "alignment must not be larger than natural")
(assert_invalid
  (module (func (table.init 0 (i32.const 12) (i32.const 1) (i32.const 1))))
  "unknown table 0")
"#;

/// Correct expectations that need the `simd` feature to parse at all.
#[cfg(feature = "simd")]
const CORRECT_EXPECTATIONS_SIMD: &str = r#"
(module (func (export "v") (result v128) (v128.const f32x4 nan:0x7fffff 0 0 0)))
(assert_return (invoke "v") (v128.const f32x4 nan:arithmetic 0 0 0))
"#;

#[test]
fn harness_rejects_wrong_spec_expectations() {
    for (name, text) in REJECTED {
        assert!(
            run_wast_outcome(text).is_err(),
            "harness accepted a wrong expectation: {name}"
        );
    }
}

#[cfg(feature = "simd")]
#[test]
fn harness_rejects_wrong_spec_expectations_simd() {
    for (name, text) in REJECTED_SIMD {
        assert!(
            run_wast_outcome(text).is_err(),
            "harness accepted a wrong expectation: {name}"
        );
    }
}

#[test]
fn harness_accepts_correct_spec_expectations() {
    assert!(
        run_wast_outcome(CORRECT_EXPECTATIONS).is_ok(),
        "harness rejected correct spec expectations"
    );
}

#[cfg(feature = "simd")]
#[test]
fn harness_accepts_correct_spec_expectations_simd() {
    assert!(
        run_wast_outcome(CORRECT_EXPECTATIONS_SIMD).is_ok(),
        "harness rejected correct spec expectations (simd)"
    );
}
