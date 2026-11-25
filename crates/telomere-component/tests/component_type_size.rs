//! Boundary tests for the Canonical ABI `elem_size` validation bound
//! (issue #323): a defined value type is valid iff its `elem_size`,
//! computed for a 64-bit pointer layout, is less than 2^28. These cases
//! mirror `validation/max-value-size.wast` from the vendored upstream
//! suite, restricted to the type shapes telomere implements (the file
//! itself stays excluded because its valid-boundary component also
//! uses map and stream/future types, which are documented non-goals).

use telomere_component::ComponentEngine;

fn compile_component(text: &str) -> Result<(), telomere_component::ComponentError> {
    let bytes = wat::parse_str(text).expect("component wat must be valid");
    let engine = ComponentEngine::new();
    engine.compile(&bytes).map(|_| ())
}

fn assert_valid(types: &str) {
    let text = format!("(component {types})");
    compile_component(&text)
        .unwrap_or_else(|error| panic!("expected {types} to be valid, got: {error}"));
}

fn assert_too_big(types: &str) {
    let text = format!("(component {types})");
    let error = compile_component(&text)
        .err()
        .unwrap_or_else(|| panic!("expected {types} to be rejected"));
    assert!(
        error.to_string().contains("maximum byte size"),
        "expected a type-size error for {types}, got: {error}"
    );
}

#[test]
fn fixed_list_boundaries_are_valid() {
    // elem_size = 268435455 = 2^28 - 1
    assert_valid("(type (list u8 268435455))");
    // elem_size = 8 * 33554431 = 268435448
    assert_valid("(type (list u64 33554431))");
    // elem_size = 16 * 16777215 = 268435440 (string is a pointer pair)
    assert_valid("(type (list string 16777215))");
}

#[test]
fn compound_sum_boundaries_are_valid() {
    // tuple is a record: 268435454 + 1 = 268435455
    assert_valid("(type (tuple (list u8 268435454) (list u8 1)))");
    // 134217727 + 134217728 = 268435455
    assert_valid(
        "(type (record \
           (field \"a\" (list u8 134217727)) \
           (field \"b\" (list u8 134217728))))",
    );
    // nested fixed list: 2 * 134217727 = 268435454
    assert_valid("(type (list (list u8 134217727) 2))");
    // referenced element type: 2 * 134217727 = 268435454
    assert_valid("(type $big (list u8 134217727)) (type (list $big 2))");
}

#[test]
fn dynamic_list_of_huge_element_type_is_valid() {
    // A dynamically-sized list is a (pointer, length) pair: elem_size
    // 16 regardless of the element type's size.
    assert_valid("(type $big (list u8 268435455)) (type (list $big))");
}

#[test]
fn record_alignment_padding_boundary_is_valid() {
    // The u64 field starts at offset 268435440 (already 8-aligned):
    // 268435440 + 8 = 268435448 < 2^28.
    assert_valid("(type (record (field \"a\" (list u8 268435440)) (field \"b\" u64)))");
}

#[test]
fn variant_boundary_is_valid() {
    // discriminant (1) + payload 268435454 = 268435455
    assert_valid("(type (variant (case \"a\") (case \"b\" (list u8 268435454))))");
}

#[test]
fn func_type_aggregates_are_not_size_gated() {
    // The elem_size bound applies to value types, not to func types:
    // each parameter type is below the bound, so the func type is
    // valid even though the parameters' sizes sum past it.
    assert_valid(
        "(type $big (list u8 200000000)) \
         (type (func (param \"a\" $big) (param \"b\" $big)))",
    );
}

#[test]
fn fixed_list_just_over_the_limit_is_rejected() {
    // elem_size = 268435456 = 2^28 exactly
    assert_too_big("(type (list u8 268435456))");
}

#[test]
fn byte_size_not_element_count_decides() {
    // 33554432 elements of 8 bytes = 2^28 bytes: over the limit even
    // though the element count is far below 2^28.
    assert_too_big("(type (list u64 33554432))");
    // 536870912 * 8 = 2^32 bytes (the u32 wrap class).
    assert_too_big("(type (list u64 536870912))");
    // 16777216 strings * 16 bytes = 2^28 under a 64-bit layout (this
    // would pass with 32-bit pointers; validation uses the 64-bit one).
    assert_too_big("(type (list string 16777216))");
}

#[test]
fn compound_sum_over_the_limit_is_rejected() {
    // 268435455 + 1 = 2^28
    assert_too_big("(type (tuple (list u8 268435455) (list u8 1)))");
    // 134217728 + 134217728 = 2^28
    assert_too_big(
        "(type (record \
           (field \"a\" (list u8 134217728)) \
           (field \"b\" (list u8 134217728))))",
    );
    // 2 * 268435455 = 536870910
    assert_too_big("(type (list (list u8 268435455) 2))");
    // referenced element type: 2 * 268435455
    assert_too_big("(type $big (list u8 268435455)) (type (list $big 2))");
}

#[test]
fn record_alignment_padding_can_push_over_the_limit() {
    // The u64 field is padded from offset 268435441 up to 268435448,
    // so the record size is 268435448 + 8 = 2^28.
    assert_too_big("(type (record (field \"a\" (list u8 268435441)) (field \"b\" u64)))");
}

#[test]
fn variant_over_the_limit_is_rejected() {
    // discriminant (1) + payload 268435455 = 2^28
    assert_too_big("(type (variant (case \"a\") (case \"b\" (list u8 268435455))))");
}

#[test]
fn shared_reference_chains_stay_fast() {
    // Regression (Codex P1 on PR #364): the alignment walk used to
    // re-expand shared type references on every visit, so a chain of
    // types whose two branches both reference the preceding type took
    // exponential time to validate even though every type in the
    // chain is small and valid (measured pre-fix in release mode:
    // 24 variant definitions / 427 bytes took ~2.1 s, doubling with
    // each added definition). Alignment is now memoised per type, so
    // the same shapes are linear; the 40-deep variant chain is only
    // feasible at all with the cache (2^40 uncached steps). The time
    // bound is a loose backstop, not the primary assertion.
    let mut variant = String::from("(type $v0 (variant (case \"x\" u8)))");
    for k in 1..40 {
        variant.push_str(&format!(
            " (type $v{k} (variant (case \"l\" $v{}) (case \"r\" $v{})))",
            k - 1,
            k - 1
        ));
    }
    // Record sizes double per level: depth 23 peaks at 2^23 bytes.
    let mut record = String::from("(type $r0 (record (field \"a\" u8)))");
    for k in 1..24 {
        record.push_str(&format!(
            " (type $r{k} (record (field \"l\" $r{}) (field \"r\" $r{})))",
            k - 1,
            k - 1
        ));
    }
    let start = std::time::Instant::now();
    assert_valid(&variant);
    assert_valid(&record);
    assert!(
        start.elapsed() < std::time::Duration::from_secs(10),
        "shared-reference chains must validate in linear time, took {:?}",
        start.elapsed()
    );
}

#[test]
fn shared_variant_chain_as_func_result_stays_fast() {
    // Regression (Codex P1 on PR #364, round 2): admitting shared type
    // graphs under the elem_size rule also admitted them as function
    // results, where the parse-time borrow check
    // (`type_contains_borrow`) re-expanded both variant branches at
    // every level without memoisation — exponential time on small,
    // valid inputs (measured pre-fix in release mode: 28 definitions /
    // ~1.5 KB took ~3.5 s as a func result, vs ~0.13 ms for the
    // definitions alone; each added definition doubled the work). The
    // predicate is now memoised per type, like the size and alignment
    // walks. A 40-deep chain is only feasible at all with the cache
    // (2^40 uncached steps); the time bound is a loose backstop.
    let mut types = String::from("(type $v0 (variant (case \"x\" u8)))");
    for k in 1..40 {
        types.push_str(&format!(
            " (type $v{k} (variant (case \"l\" $v{}) (case \"r\" $v{})))",
            k - 1,
            k - 1
        ));
    }
    types.push_str(" (type $f (func (result $v39)))");
    let start = std::time::Instant::now();
    assert_valid(&types);
    assert!(
        start.elapsed() < std::time::Duration::from_secs(10),
        "borrow check on a shared variant chain must be linear, took {:?}",
        start.elapsed()
    );
}

#[test]
fn borrow_deep_in_shared_chain_is_still_rejected() {
    // The memoised borrow check must keep its semantics: a `borrow`
    // reachable through a shared variant chain still makes a function
    // result invalid, while the `own` mirror of the same chain is
    // accepted.
    let mut borrow_chain = String::from(
        "(type $r (resource (rep i32))) (type $b (borrow $r)) (type $v0 (variant (case \"x\" $b)))",
    );
    for k in 1..8 {
        borrow_chain.push_str(&format!(
            " (type $v{k} (variant (case \"l\" $v{}) (case \"r\" $v{})))",
            k - 1,
            k - 1
        ));
    }
    borrow_chain.push_str(" (type $f (func (result $v7)))");
    let text = format!("(component {borrow_chain})");
    let error = compile_component(&text)
        .err()
        .unwrap_or_else(|| panic!("expected the borrow chain result to be rejected"));
    assert!(
        error.to_string().contains("cannot contain a `borrow` type"),
        "expected a borrow-in-result error, got: {error}"
    );

    let mut own_chain = String::from(
        "(type $r (resource (rep i32))) (type $o (own $r)) (type $v0 (variant (case \"x\" $o)))",
    );
    for k in 1..8 {
        own_chain.push_str(&format!(
            " (type $v{k} (variant (case \"l\" $v{}) (case \"r\" $v{})))",
            k - 1,
            k - 1
        ));
    }
    own_chain.push_str(" (type $f (func (result $v7)))");
    assert_valid(&own_chain);
}

#[test]
fn nested_type_definitions_are_size_gated() {
    // Regression (Codex P2 on PR #364, round 2): the elem_size gate
    // applies to defined value types, but types defined *inside* an
    // instance or component type were registered without ever running
    // it — the aggregate traversal that used to catch oversized types
    // through their exports no longer runs. Both nested forms must
    // reject a value type at the 2^28 boundary, and accept the same
    // shape one byte below it.
    assert_too_big(
        "(type (instance (type $big (list u8 268435456)) (export \"big\" (type (eq $big)))))",
    );
    assert_too_big(
        "(type (component (type $big (list u8 268435456)) (export \"big\" (type (eq $big)))))",
    );
    assert_valid(
        "(type (instance (type $big (list u8 268435455)) (export \"big\" (type (eq $big)))))",
    );
    assert_valid(
        "(type (component (type $big (list u8 268435455)) (export \"big\" (type (eq $big)))))",
    );
}
