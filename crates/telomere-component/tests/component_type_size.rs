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
