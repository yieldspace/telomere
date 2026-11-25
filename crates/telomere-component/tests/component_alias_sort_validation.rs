//! Regression tests for issue #356: binary alias sort validation gaps
//! found by the vendored upstream conformance suite
//! (`binary/binary.wast` lines 460 and 924).
//!
//! - A component *outer* alias naming a sort outside `outeraliassort`
//!   (core module, core type, component, type) was accepted when the
//!   aliased item existed in the target scope — e.g. an `instance`
//!   outer alias (binary.wast:460). The sort is now rejected as
//!   malformed with the reference decoder's message.
//! - A core alias inside a module type whose sort byte is not 0x10
//!   was rejected with a non-canonical paraphrase
//!   (`unsupported core module alias sort: 0`) instead of the
//!   canonical `invalid leading byte (0x0) for outer alias kind`
//!   (binary.wast:924); the target byte is likewise reported as
//!   `invalid leading byte (0x..) for outer alias target`.
//!
//! The negative cases are raw binaries mirroring the upstream
//! `assert_malformed` cases, because the text format's grammar cannot
//! even express an out-of-set outer-alias sort. The positive cases go
//! through the text format and pin that every sort in
//! `outeraliassort` stays accepted.

use telomere_component::ComponentEngine;

/// `\0asm` + version `0x0d 0x00` + layer `0x01 0x00`.
const PREAMBLE: &[u8] = b"\0asm\x0d\0\x01\0";

fn compile(bytes: &[u8]) -> Result<(), String> {
    let engine = ComponentEngine::new();
    engine.compile(bytes).map(|_| ()).map_err(|e| e.to_string())
}

fn component(sections: &[u8]) -> Vec<u8> {
    let mut bytes = PREAMBLE.to_vec();
    bytes.extend_from_slice(sections);
    bytes
}

fn expect_malformed(bytes: &[u8], reason: &str) {
    let error = compile(bytes).expect_err("malformed component must be rejected");
    assert!(
        error.contains(reason),
        "expected an error containing `{reason}`, got `{error}`"
    );
    // A sort that cannot start an outer alias is a framing problem,
    // so the public category is a decode error.
    assert!(
        error.starts_with("decode error:"),
        "expected a decode error, got: {error}"
    );
}

fn compile_text(text: &str) -> Result<(), String> {
    let bytes = wat::parse_str(text).expect("component wat must be valid");
    compile(&bytes)
}

fn expect_valid_text(text: &str) {
    compile_text(text).expect("component must be accepted");
}

// ---------------------------------------------------------------------
// binary/binary.wast:460 — component outer aliases
// ---------------------------------------------------------------------

#[test]
fn outer_alias_of_instance_sort_is_rejected() {
    // The exact upstream case: an instance section defines one
    // instance, then an alias names the instance sort with the outer
    // target. `instance` is not in `outeraliassort`, so the alias is
    // malformed even though instance 0 exists in the current scope.
    let bytes = component(&[
        0x05, 0x03, 0x01, 0x01, 0x00, // instance section: 1 inline instance, 0 exports
        0x06, 0x05, 0x01, 0x05, 0x02, 0x00, 0x00, // alias: sort=instance, outer, ct=0, idx=0
    ]);
    expect_malformed(
        &bytes,
        "invalid leading byte (0x5) for component outer alias kind",
    );
}

#[test]
fn outer_alias_of_func_sort_is_rejected() {
    // Alias section only: sort=func (0x01), outer target, ct=0, idx=0.
    let bytes = component(&[0x06, 0x05, 0x01, 0x01, 0x02, 0x00, 0x00]);
    expect_malformed(
        &bytes,
        "invalid leading byte (0x1) for component outer alias kind",
    );
}

#[test]
fn outer_alias_of_value_sort_is_rejected() {
    // Alias section only: sort=value (0x02), outer target, ct=0, idx=0.
    let bytes = component(&[0x06, 0x05, 0x01, 0x02, 0x02, 0x00, 0x00]);
    expect_malformed(
        &bytes,
        "invalid leading byte (0x2) for component outer alias kind",
    );
}

#[test]
fn outer_alias_of_core_func_sort_is_rejected() {
    // Sort = core func (0x00 prefix, core byte 0x00): the diagnostic
    // names the core sort byte, as in binary.wast lines 447/456.
    let bytes = component(&[0x06, 0x06, 0x01, 0x00, 0x00, 0x02, 0x00, 0x00]);
    expect_malformed(
        &bytes,
        "invalid leading byte (0x0) for component outer alias kind",
    );
}

#[test]
fn outer_alias_of_core_instance_sort_is_rejected() {
    // Sort = core instance (0x00 prefix, core byte 0x12).
    let bytes = component(&[0x06, 0x06, 0x01, 0x00, 0x12, 0x02, 0x00, 0x00]);
    expect_malformed(
        &bytes,
        "invalid leading byte (0x12) for component outer alias kind",
    );
}

#[test]
fn outer_alias_of_type_sort_is_accepted() {
    expect_valid_text(
        r#"(component
             (type $t (record (field "x" u32)))
             (component $c
               (alias outer 1 $t (type $t2))))"#,
    );
}

#[test]
fn outer_alias_of_component_sort_is_accepted() {
    expect_valid_text(
        r#"(component
             (component $c1)
             (component $c2
               (alias outer 1 $c1 (component $c3))))"#,
    );
}

#[test]
fn outer_alias_of_core_type_sort_is_accepted() {
    expect_valid_text(
        r#"(component
             (core type $ct (func))
             (component $c
               (alias outer 1 $ct (core type $ct2))))"#,
    );
}

#[test]
fn outer_alias_of_core_module_sort_is_accepted() {
    expect_valid_text(
        r#"(component
             (core module $m)
             (component $c
               (alias outer 1 $m (core module $m2))))"#,
    );
}

// ---------------------------------------------------------------------
// binary/binary.wast:924 — core aliases inside a module type
// ---------------------------------------------------------------------

#[test]
fn core_module_alias_with_func_sort_is_rejected() {
    // The exact upstream case: core type section holding one module
    // type with one alias decl whose sort byte is 0x00, not 0x10.
    let bytes = component(&[
        0x03, 0x08, 0x01, 0x50, 0x01, // core type section: 1 moduletype, 1 decl
        0x02, 0x00, 0x01, 0x01, 0x00, // alias: sort=0x00, outer, ct=1, idx=0
    ]);
    expect_malformed(&bytes, "invalid leading byte (0x0) for outer alias kind");
}

#[test]
fn core_module_alias_with_module_sort_is_rejected() {
    // Same shape with sort byte 0x11 (module): still not the fixed
    // 0x10 (type) sort of `core:alias`.
    let bytes = component(&[
        0x03, 0x08, 0x01, 0x50, 0x01, // core type section: 1 moduletype, 1 decl
        0x02, 0x11, 0x01, 0x01, 0x00, // alias: sort=0x11, outer, ct=1, idx=0
    ]);
    expect_malformed(&bytes, "invalid leading byte (0x11) for outer alias kind");
}

#[test]
fn core_module_alias_with_non_outer_target_is_rejected() {
    // Sort byte is the fixed 0x10, but the target byte is 0x00, not
    // the fixed 0x01 (outer) of `core:alias` (binary.wast, the case
    // following line 924).
    let bytes = component(&[
        0x03, 0x08, 0x01, 0x50, 0x01, // core type section: 1 moduletype, 1 decl
        0x02, 0x10, 0x00, 0x01, 0x00, // alias: sort=0x10, target=0x00, ct=1, idx=0
    ]);
    expect_malformed(&bytes, "invalid leading byte (0x0) for outer alias target");
}

#[test]
fn core_module_alias_with_type_sort_is_accepted() {
    // Core type section: type 0 is a core functype; type 1 is a
    // module type whose single decl aliases outer core type 0.
    let bytes = component(&[
        0x03, 0x0b, 0x02, // core type section: 2 types
        0x60, 0x00, 0x00, // type 0: (func)
        0x50, 0x01, // type 1: moduletype, 1 decl
        0x02, 0x10, 0x01, 0x01, 0x00, // alias: sort=type, outer, ct=1, idx=0
    ]);
    compile(&bytes).expect("a core alias naming the type sort must be accepted");
}
