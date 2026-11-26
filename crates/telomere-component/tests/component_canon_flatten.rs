//! Regression tests for issue #367: the canon layer re-expanded
//! shared type references at every visit, so flattening a type graph
//! (and the `needs memory` predicate asked alongside it) took
//! exponential time on small, valid inputs.
//!
//! The shape is a chain of variants whose two cases both reference
//! the preceding type, with the chain's final type used as a canon
//! lift parameter. Measured pre-fix in release mode on current main:
//! 24 definitions took ~1.2 s to compile and the time doubled with
//! each added definition, even though the component is ultimately
//! rejected (the flattened parameter list exceeds the flat limit, so
//! a `memory` option is required) — the exponential time was spent in
//! `decoder/canon/flatten.rs` and the `type_needs_memory` walk in
//! `decoder/canon/options.rs` before that rejection. Both walks are
//! now memoised per type; a 40-deep chain is only feasible at all
//! with the caches (2^40 uncached steps). The time bound is a loose
//! backstop, not the primary assertion.
//!
//! The memoised flat representation is also bounded: each type
//! retains at most `MAX_FLAT_PARAMS + 1` values, since every consumer
//! only distinguishes lengths up to the flat limit. Before the bound,
//! a wide type behind a chain of single-field wrappers made every
//! wrapper retain a complete copy of the wide representation (Codex
//! probe on PR #372: 20 doubling tuple definitions plus 128 wrappers
//! peaked at ~71 MB resident versus ~5 MB without the wrappers,
//! measured locally with this shape). The wrapper test below uses
//! 512 wrappers, which would retain ~512 full copies unbounded; it
//! pins the bound by completion and by the unchanged rejection, not
//! by a wall-clock or resident-memory threshold.

use telomere_component::ComponentEngine;

fn compile_component(text: &str) -> Result<(), telomere_component::ComponentError> {
    let bytes = wat::parse_str(text).expect("component wat must be valid");
    let engine = ComponentEngine::new();
    engine.compile(&bytes).map(|_| ())
}

fn variant_chain(depth: usize, leaf: &str) -> String {
    let mut types = format!("(type $v0 (variant (case \"x\" {leaf})))");
    for k in 1..depth {
        types.push_str(&format!(
            " (type $v{k} (variant (case \"l\" $v{}) (case \"r\" $v{})))",
            k - 1,
            k - 1
        ));
    }
    types
}

fn lift_component(types: &str, core_params: &str, param_ty: &str) -> String {
    format!(
        "(component\n  (core module $m (func (export \"f\") (param {core_params}) unreachable))\n  (core instance $i (instantiate $m))\n  {types}\n  (func $f (param \"p\" {param_ty}) (canon lift (core func $i \"f\"))))"
    )
}

#[test]
fn shared_variant_chain_as_lift_param_stays_fast() {
    let types = variant_chain(40, "u8");
    let text = lift_component(&types, "i32", "$v39");
    let start = std::time::Instant::now();
    let error = compile_component(&text)
        .err()
        .unwrap_or_else(|| panic!("expected the 40-deep chain lift to be rejected"));
    assert!(
        start.elapsed() < std::time::Duration::from_secs(10),
        "canon flatten of a shared variant chain must be linear, took {:?}",
        start.elapsed()
    );
    // The rejection itself is unchanged: 41 flat parameters exceed
    // the limit, so a memory option is required.
    assert!(
        error
            .to_string()
            .contains("canonical option `memory` is required"),
        "expected a memory-required error, got: {error}"
    );
}

#[test]
fn shared_chain_flattens_to_the_joined_signature() {
    // flat($a) = [i32, f32] (discriminant + payload). $b's payload
    // joins $a's flat [i32, f32] with f64's [f64] into [i64, f32], so
    // flat($b) = [i32, i64, f32]; $c references $b twice, so
    // flat($c) = [i32, i32, i64, f32]. The memoised walk must produce
    // exactly the uncached result.
    let types = "(type $a (variant (case \"x\" f32))) \
                 (type $b (variant (case \"l\" $a) (case \"r\" f64))) \
                 (type $c (variant (case \"l\" $b) (case \"r\" $b)))";
    let text = lift_component(types, "i32 i32 i64 f32", "$c");
    compile_component(&text)
        .unwrap_or_else(|error| panic!("expected the joined lift to compile, got: {error}"));
}

#[test]
fn shallow_shared_chain_lifts_without_memory() {
    // A 15-deep u8 chain flattens to 16 i32 parameters (a
    // discriminant plus the joined payload per level; exactly the
    // flat limit) and contains no memory-needing type, so it lifts
    // with a matching core function and no options. This pins the
    // memoised `needs memory` predicate returning false through
    // shared references.
    let types = variant_chain(15, "u8");
    let core_params = vec!["i32"; 16].join(" ");
    let text = lift_component(&types, &core_params, "$v14");
    compile_component(&text)
        .unwrap_or_else(|error| panic!("expected the shallow chain to compile, got: {error}"));
}

#[test]
fn memory_deep_in_shared_chain_is_still_detected() {
    // The memoised predicate must keep its semantics: a `string`
    // leaf deep in a shared chain still makes the lift require a
    // memory, even though the flattened form is small.
    let types = variant_chain(6, "string");
    let text = lift_component(&types, "i32 i32 i32", "$v5");
    let error = compile_component(&text)
        .err()
        .unwrap_or_else(|| panic!("expected the string chain lift to be rejected"));
    assert!(
        error
            .to_string()
            .contains("canonical option `memory` is required"),
        "expected a memory-required error, got: {error}"
    );
}

fn doubling_tuples(depth: usize) -> String {
    let mut types = "(type $t0 (tuple u8))".to_string();
    for k in 1..depth {
        types.push_str(&format!(" (type $t{k} (tuple $t{} $t{}))", k - 1, k - 1));
    }
    types
}

#[test]
fn wide_type_behind_many_wrappers_stays_bounded() {
    // $t19 flattens to 2^19 values. Each of the 512 single-field
    // wrappers around it has the same flattened form; with an
    // unbounded per-type cache every wrapper would retain a complete
    // copy. The bounded cache retains a prefix of at most
    // MAX_FLAT_PARAMS + 1 values per type, so this completes with
    // the same rejection as the bare chain: the flattened parameter
    // list exceeds the flat limit, so a memory option is required.
    let mut types = doubling_tuples(20);
    types.push_str(" (type $w0 (tuple $t19))");
    for k in 1..512 {
        types.push_str(&format!(" (type $w{k} (tuple $w{}))", k - 1));
    }
    let text = lift_component(&types, "i32", "$w511");
    let error = compile_component(&text)
        .err()
        .unwrap_or_else(|| panic!("expected the wrapped wide lift to be rejected"));
    assert!(
        error
            .to_string()
            .contains("canonical option `memory` is required"),
        "expected a memory-required error, got: {error}"
    );
}

#[test]
fn at_limit_chain_behind_wrappers_lifts_with_exact_signature() {
    // The 15-deep u8 variant chain flattens to exactly the flat
    // limit (16 values). Wrapping it in single-field tuples must not
    // change the flattened form: the bounded cache stores exact
    // prefixes below the bound, so the wrapped type still lifts
    // against a core function with 16 i32 parameters and no memory.
    let mut types = variant_chain(15, "u8");
    types.push_str(" (type $w0 (tuple $v14))");
    types.push_str(" (type $w1 (tuple $w0))");
    types.push_str(" (type $w2 (tuple $w1))");
    let core_params = vec!["i32"; 16].join(" ");
    let text = lift_component(&types, &core_params, "$w2");
    compile_component(&text)
        .unwrap_or_else(|error| panic!("expected the wrapped chain to compile, got: {error}"));
}
