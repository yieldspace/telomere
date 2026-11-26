//! Regression tests for the fifth PR #384 Codex review round: two
//! validator sites that still resolved an `(eq)` face on one side
//! only.
//!
//! 1. `decoder/validator/subtyping.rs::assert_type_ids_subtype_of`
//!    resolved only the *parent's* equality chain (round 2, fix 5),
//!    comparing the denoted type against the child's unresolved
//!    face. With one imported abstract resource plainly exported
//!    twice, the two faces never met: a function returning
//!    `(own $a)` could not be export-ascribed as returning
//!    `(own $b)`, and the fall-through type-level comparison
//!    rejected with `sub resource cannot assign to except sub
//!    resource`. Both chains are now resolved to their roots
//!    before comparing.
//!
//! 2. `decoder/validator/validate_types.rs::validate_eq_mention`
//!    rejected every plain export face mentioned away from its own
//!    surface, whatever the denoted type. A face of a primitive or
//!    of an anonymous container needs no name at all -- the
//!    external-visibility rule only names resources and named
//!    defvals -- so an import whose func type mentions the face of
//!    a `u32` (or of a `(list u32)`) was rejected with
//!    `surface type requires exported/imported name` even though
//!    the denoted type requires no imported name. The mention now
//!    judges the denoted type's own naming requirement: nameless
//!    denoted types are validated through their definition (whose
//!    contents are then judged one by one), and faces of named
//!    types keep the rejection.
//!
//! Shapes that fail on the pre-fix tree are marked as such; the
//! controls pass both ways.

use telomere_component::ComponentEngine;

fn expect_ok(text: &str) {
    let bytes = wat::parse_str(text).expect("component wat must be valid");
    let engine = ComponentEngine::new();
    engine.compile(&bytes).expect("component must compile");
}

fn expect_invalid_with(text: &str, needle: &str) {
    let bytes = wat::parse_str(text).expect("component wat must be valid");
    let engine = ComponentEngine::new();
    let error = engine
        .compile(&bytes)
        .expect_err("component must be rejected");
    let message = error.to_string();
    assert!(
        message.contains(needle),
        "expected error containing `{needle}`, got `{message}`"
    );
}

/// A canon-lifted function whose declared result is
/// `(own <declared>)`, export-ascribed as `(own <ascribed>)`.
/// The core func returns a dummy i32; only the validation-time
/// ascription comparison is exercised.
fn own_result_component(declared: &str, ascribed: &str) -> String {
    format!(
        r#"
(component
  (import "r" (type $r (sub resource)))
  (import "s" (type $s (sub resource)))
  (export $a "a" (type $r))
  (export $b "b" (type $r))
  (export $c "c" (type $s))
  (export $d "d" (type $a))
  (core module $m (func (export "f") (result i32) i32.const 0))
  (core instance $i (instantiate $m))
  (func $f (result (own {declared})) (canon lift (core func $i "f")))
  (export "g" (func $f) (func (result (own {ascribed})))))
"#
    )
}

/// A canon-lifted function whose declared parameter is
/// `({kind} <declared>)`, export-ascribed with
/// `({kind} <ascribed>)`.
fn param_component(kind: &str, declared: &str, ascribed: &str) -> String {
    format!(
        r#"
(component
  (import "r" (type $r (sub resource)))
  (export $a "a" (type $r))
  (export $b "b" (type $r))
  (core module $m (func (export "f") (param i32) (result i32) (local.get 0)))
  (core instance $i (instantiate $m))
  (func $f (param "x" ({kind} {declared})) (result u32) (canon lift (core func $i "f")))
  (export "g" (func $f) (func (param "x" ({kind} {ascribed})) (result u32))))
"#
    )
}

// ---------------------------------------------------------------------
// Finding 1: both equality chains resolved in subtype comparisons.
// ---------------------------------------------------------------------

/// Fails pre-fix: the two faces of one imported resource never met.
#[test]
fn two_faces_of_one_resource_agree_in_result() {
    expect_ok(&own_result_component("$a", "$b"));
}

/// Fails pre-fix: the same comparison in the parameter
/// (contravariant) direction.
#[test]
fn two_faces_of_one_resource_agree_in_parameter() {
    expect_ok(&param_component("own", "$a", "$b"));
}

/// Fails pre-fix: borrow handles compare through the same path.
#[test]
fn two_faces_of_one_resource_agree_for_borrow_parameter() {
    expect_ok(&param_component("borrow", "$a", "$b"));
}

/// Fails pre-fix: a chain of faces on the declared side still
/// resolves to the common resource root.
#[test]
fn face_chain_and_face_of_one_resource_agree() {
    expect_ok(&own_result_component("$d", "$b"));
}

/// Fails pre-fix: the variant-presentation shape of round 3, but
/// with the resource imported abstractly and named through two
/// different faces on the declared and ascribed sides.
#[test]
fn two_faces_agree_in_variant_presentation_shape() {
    expect_ok(
        r#"
(component
  (import "r" (type $r (sub resource)))
  (export $a "a" (type $r))
  (export $b "b" (type $r))
  (type $v (variant (case "none") (case "some" (own $a))))
  (core module $m
    (memory (export "memory") 1)
    (func (export "realloc") (param i32 i32 i32 i32) (result i32) i32.const 1024)
    (func (export "none") (result i32) i32.const 64))
  (core instance $i (instantiate $m))
  (func $none (result $v)
    (canon lift (core func $i "none") (memory (core memory $i "memory")) (realloc (core func $i "realloc"))))
  (export "none" (func $none) (func (result (option (own $b))))))
"#,
    );
}

/// Control: the denoted resource itself on the declared side and
/// its face on the ascribed side already agreed pre-fix (the old
/// loop resolved the parent's chain onto the child).
#[test]
fn declared_resource_ascribed_through_its_face() {
    expect_ok(&own_result_component("$r", "$b"));
}

/// Control: the same face on both sides.
#[test]
fn same_face_on_both_sides_of_ascription() {
    expect_ok(&own_result_component("$a", "$a"));
}

/// Control: faces of genuinely distinct abstract resources stay
/// rejected, with the pre-existing message.
#[test]
fn faces_of_distinct_resources_stay_rejected() {
    expect_invalid_with(
        &own_result_component("$a", "$c"),
        "sub resource cannot assign to except sub resource",
    );
}

// ---------------------------------------------------------------------
// Finding 2: anonymous denoted types need no name at a surface.
// ---------------------------------------------------------------------

/// An import whose func type mentions the face of the given
/// denoted type definition.
fn import_mention_component(type_def: &str, mention: &str) -> String {
    format!(
        r#"
(component
  (type $t {type_def})
  (export $a "a" (type $t))
  (export $b "b" (type $a))
  (import "f" (func (param "x" {mention}))))
"#
    )
}

/// Fails pre-fix: a plain-exported `u32` mentioned by an import.
#[test]
fn face_of_primitive_in_import_param() {
    expect_ok(&import_mention_component("u32", "$a"));
}

/// Fails pre-fix: a plain-exported `(list u32)`.
#[test]
fn face_of_list_of_primitive_in_import_param() {
    expect_ok(&import_mention_component("(list u32)", "$a"));
}

/// Fails pre-fix: anonymous containers nest.
#[test]
fn face_of_nested_list_in_import_param() {
    expect_ok(&import_mention_component("(list (list u32))", "$a"));
}

/// Fails pre-fix: a chain of two faces of a primitive resolves to
/// the denoted type's (absent) naming requirement.
#[test]
fn face_chain_of_primitive_in_import_param() {
    expect_ok(&import_mention_component("u32", "$b"));
}

/// Fails pre-fix: the face mentioned nested inside an anonymous
/// container written on the import surface itself.
#[test]
fn face_of_primitive_nested_in_import_param() {
    expect_ok(
        r#"
(component
  (type $t u32)
  (export $a "a" (type $t))
  (import "f" (func (param "x" (list $a)))))
"#,
    );
}

/// Fails pre-fix: result position judges the same way.
#[test]
fn face_of_primitive_in_import_result() {
    expect_ok(
        r#"
(component
  (type $t string)
  (export $a "a" (type $t))
  (import "f" (func (result $a))))
"#,
    );
}

/// Control: a face of a resource mentioned on an import surface
/// stays rejected -- the denoted type needs a name, and imports
/// may not depend on exports.
#[test]
fn face_of_resource_in_import_stays_rejected() {
    expect_invalid_with(
        r#"
(component
  (type $r (resource (rep i32)))
  (export $a "a" (type $r))
  (import "f" (func (param "x" (own $a)))))
"#,
        "surface type requires exported/imported name",
    );
}

/// Control: a face of a named record stays rejected.
#[test]
fn face_of_named_record_in_import_stays_rejected() {
    expect_invalid_with(
        &import_mention_component("(record (field \"x\" u32))", "$a"),
        "surface type requires exported/imported name",
    );
}

/// Control: a face of a flags type stays rejected (flags are
/// never anonymous at a surface).
#[test]
fn face_of_flags_in_import_stays_rejected() {
    expect_invalid_with(
        &import_mention_component("(flags \"a\" \"b\")", "$a"),
        "surface type requires exported/imported name",
    );
}

/// Control: an anonymous container whose *contents* include a
/// named type stays rejected -- the denoted type needs no name
/// itself, but the definition walk judges its contents one by
/// one, and the named record inside has no name on this surface.
#[test]
fn face_of_list_of_named_record_in_import_stays_rejected() {
    expect_invalid_with(
        r#"
(component
  (type $rec (record (field "x" u32)))
  (type $t (list $rec))
  (export $a "a" (type $t))
  (import "f" (func (param "x" $a))))
"#,
        "surface type requires exported/imported name",
    );
}
