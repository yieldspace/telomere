//! Regression tests for #359: binary-form extern names must be
//! validated like every other extern name. The vendored upstream
//! `binary/binary.wast` cases at lines 1467 and 1481 found two gaps:
//!
//! - a plain import name that is not kebab-case (`Foo`) was accepted
//!   for non-instance sorts (the surface name check only ran for
//!   instance imports/exports), and
//! - an empty import name was rejected, but with the doubled,
//!   non-canonical message `Invalid import name: Invalid import
//!   name: ``` instead of `` `` is not a valid extern name ``.
//!
//! The canonical message is the one wasmparser's `validate_extern`
//! wraps every name failure in: `` `{name}` is not a valid extern
//! name ``.

use telomere_component::ComponentEngine;

fn compile_bytes(bytes: &[u8]) -> Result<(), telomere_component::ComponentError> {
    let engine = ComponentEngine::new();
    engine.compile(bytes).map(|_| ())
}

fn compile_text(text: &str) -> Result<(), telomere_component::ComponentError> {
    let bytes = wat::parse_str(text).expect("component wat must be valid");
    compile_bytes(&bytes)
}

fn assert_invalid_bytes(bytes: &[u8], expected: &str) {
    match compile_bytes(bytes) {
        Ok(()) => panic!("expected an error containing `{expected}`, but compilation succeeded"),
        Err(error) => {
            let actual = error.to_string();
            assert!(
                actual.contains(expected),
                "expected an error containing `{expected}`, got `{actual}`"
            );
        }
    }
}

fn assert_invalid_text(text: &str, expected: &str) {
    match compile_text(text) {
        Ok(()) => panic!("expected an error containing `{expected}`, but compilation succeeded"),
        Err(error) => {
            let actual = error.to_string();
            assert!(
                actual.contains(expected),
                "expected an error containing `{expected}`, got `{actual}`"
            );
        }
    }
}

fn assert_valid_text(text: &str) {
    if let Err(error) = compile_text(text) {
        panic!("expected a valid component, got error: {error}");
    }
}

/// The exact byte sequence of the upstream `binary/binary.wast`
/// line 1467 case: a func import whose plain name is `Foo`.
fn binary_wast_foo_import() -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"\0asm\x0d\0\x01\0"); // preamble
    bytes.extend_from_slice(&[0x07, 0x05]); // type section (5 bytes)
    bytes.extend_from_slice(&[0x01, 0x40, 0x00, 0x01, 0x00]); // 1 type: (func)
    bytes.extend_from_slice(&[0x0a, 0x08]); // import section (8 bytes)
    bytes.extend_from_slice(&[0x01]); // 1 import
    bytes.extend_from_slice(&[0x00]); // plain externname
    bytes.extend_from_slice(&[0x03]);
    bytes.extend_from_slice(b"Foo"); // name "Foo": not kebab-case
    bytes.extend_from_slice(&[0x01, 0x00]); // func (type 0)
    bytes
}

/// The exact byte sequence of the upstream `binary/binary.wast`
/// line 1481 case: a func import whose plain name is empty.
fn binary_wast_empty_import() -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"\0asm\x0d\0\x01\0"); // preamble
    bytes.extend_from_slice(&[0x07, 0x05]); // type section (5 bytes)
    bytes.extend_from_slice(&[0x01, 0x40, 0x00, 0x01, 0x00]); // 1 type: (func)
    bytes.extend_from_slice(&[0x0a, 0x05]); // import section (5 bytes)
    bytes.extend_from_slice(&[0x01]); // 1 import
    bytes.extend_from_slice(&[0x00]); // plain externname
    bytes.extend_from_slice(&[0x00]); // empty name
    bytes.extend_from_slice(&[0x01, 0x00]); // func (type 0)
    bytes
}

#[test]
fn binary_non_kebab_import_name_is_rejected() {
    assert_invalid_bytes(
        &binary_wast_foo_import(),
        "`Foo` is not a valid extern name",
    );
}

#[test]
fn binary_empty_import_name_uses_canonical_message() {
    assert_invalid_bytes(&binary_wast_empty_import(), "is not a valid extern name");
}

#[test]
fn non_kebab_import_names_are_rejected_for_every_sort() {
    assert_invalid_text(
        "(component (import \"Foo\" (func)))",
        "`Foo` is not a valid extern name",
    );
    assert_invalid_text(
        "(component (import \"aBc\" (func)))",
        "`aBc` is not a valid extern name",
    );
    assert_invalid_text(
        "(component (import \"a-\" (func)))",
        "is not a valid extern name",
    );
    assert_invalid_text(
        "(component (import \"1\" (func)))",
        "is not a valid extern name",
    );
    assert_invalid_text(
        "(component (import \"\" (func)))",
        "is not a valid extern name",
    );
    assert_invalid_text(
        "(component (import \"DOWn\" (instance)))",
        "is not a valid extern name",
    );
    assert_invalid_text(
        "(component (import \"Foo\" (type (sub resource))))",
        "is not a valid extern name",
    );
}

#[test]
fn non_kebab_export_name_is_rejected() {
    assert_invalid_text(
        "(component
           (import \"i\" (func $f))
           (export \"Foo\" (func $f)))",
        "`Foo` is not a valid extern name",
    );
    assert_valid_text(
        "(component
           (import \"i\" (func $f))
           (export \"foo\" (func $f)))",
    );
}

#[test]
fn kebab_import_names_stay_valid() {
    // The valid name set of upstream `validation/kebab.wast`.
    assert_valid_text(
        "(component
           (import \"a\" (func))
           (import \"a1\" (func))
           (import \"a-1-b-2-c-3\" (func))
           (import \"B\" (func))
           (import \"B1\" (func))
           (import \"B-1-C-2-D-3\" (func))
           (import \"a11-B11-123-ABC-abc\" (func)))",
    );
    // Resource type imports in the vendored suite use single-case
    // labels with digits (`T1`); they must stay valid.
    assert_valid_text("(component (import \"T1\" (type (sub resource))))");
}
