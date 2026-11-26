//! Regression tests for the fourth PR #384 Codex review round:
//! `decoder/instance.rs::parse_instantiate` accepted any
//! `Type::Generic` as the argument for a `(sub resource)` type
//! import, so a plain type export's `(eq)` face denoting a
//! primitive (or any other non-resource type) satisfied a resource
//! import. The denoted type's kind must be checked after resolving
//! the face.

use telomere_component::{ComponentEngine, ComponentError};

fn compile_component(text: &str) -> Result<(), ComponentError> {
    let bytes = wat::parse_str(text).expect("component wat must be valid");
    let engine = ComponentEngine::new();
    engine.compile(&bytes).map(|_| ())
}

fn expect_valid(text: &str) {
    compile_component(text).expect("component must be accepted");
}

fn expect_invalid_with(text: &str, needle: &str) {
    let error = compile_component(text).expect_err("component must be rejected");
    let message = error.to_string();
    assert!(
        message.contains(needle),
        "expected error containing `{needle}`, got `{message}`"
    );
}

/// The Codex finding: a plain-exported `u32` passed, through its
/// export index (the `(eq)` face), to a nested component's
/// `(sub resource)` import.
#[test]
fn faced_u32_cannot_satisfy_resource_import() {
    expect_invalid_with(
        r#"
(component
  (type $t u32)
  (export $a "a" (type $t))
  (component $inner
    (import "r" (type $r (sub resource))))
  (instance (instantiate $inner (with "r" (type $a)))))
"#,
        "expected resource",
    );
}

/// Control: the definition index of the same `u32` type, passed
/// directly, was already rejected before faces existed.
#[test]
fn direct_u32_cannot_satisfy_resource_import() {
    expect_invalid_with(
        r#"
(component
  (type $t u32)
  (component $inner
    (import "r" (type $r (sub resource))))
  (instance (instantiate $inner (with "r" (type $t)))))
"#,
        "expected resource",
    );
}

/// A plain-exported record type is a `DefVal` behind the face; it
/// is not a resource either.
#[test]
fn faced_record_cannot_satisfy_resource_import() {
    expect_invalid_with(
        r#"
(component
  (type $rec (record (field "x" u32)))
  (export $a "a" (type $rec))
  (component $inner
    (import "r" (type $r (sub resource))))
  (instance (instantiate $inner (with "r" (type $a)))))
"#,
        "expected resource",
    );
}

/// An `(eq ...)`-ascribed export of a `u32` type denotes the same
/// primitive through an equality generic; it must not satisfy a
/// resource import either.
#[test]
fn eq_ascribed_u32_cannot_satisfy_resource_import() {
    expect_invalid_with(
        r#"
(component
  (type $t u32)
  (export $a "a" (type $t) (type (eq $t)))
  (component $inner
    (import "r" (type $r (sub resource))))
  (instance (instantiate $inner (with "r" (type $a)))))
"#,
        "expected resource",
    );
}

/// A face of a face: a plain export of a plain export of `u32`.
#[test]
fn chained_face_of_u32_cannot_satisfy_resource_import() {
    expect_invalid_with(
        r#"
(component
  (type $t u32)
  (export $a "a" (type $t))
  (export $b "b" (type $a))
  (component $inner
    (import "r" (type $r (sub resource))))
  (instance (instantiate $inner (with "r" (type $b)))))
"#,
        "expected resource",
    );
}

/// Control: a plain-exported local resource passed through its
/// face is a legitimate argument for a `(sub resource)` import.
#[test]
fn faced_local_resource_satisfies_resource_import() {
    expect_valid(
        r#"
(component
  (type $r (resource (rep i32)))
  (export $a "a" (type $r))
  (component $inner
    (import "r" (type $ri (sub resource))))
  (instance (instantiate $inner (with "r" (type $a)))))
"#,
    );
}

/// Control: a plain-exported imported `(sub resource)` passed
/// through its face is legitimate.
#[test]
fn faced_imported_sub_resource_satisfies_resource_import() {
    expect_valid(
        r#"
(component
  (import "r" (type $r (sub resource)))
  (export $a "a" (type $r))
  (component $inner
    (import "r" (type $ri (sub resource))))
  (instance (instantiate $inner (with "r" (type $a)))))
"#,
    );
}

/// Control: a face of a face of a local resource is legitimate.
#[test]
fn chained_face_of_resource_satisfies_resource_import() {
    expect_valid(
        r#"
(component
  (type $r (resource (rep i32)))
  (export $a "a" (type $r))
  (export $b "b" (type $a))
  (component $inner
    (import "r" (type $ri (sub resource))))
  (instance (instantiate $inner (with "r" (type $b)))))
"#,
    );
}

/// Control: the definition index of a local resource, passed
/// directly, stays accepted.
#[test]
fn direct_resource_satisfies_resource_import() {
    expect_valid(
        r#"
(component
  (type $r (resource (rep i32)))
  (component $inner
    (import "r" (type $ri (sub resource))))
  (instance (instantiate $inner (with "r" (type $r)))))
"#,
    );
}

/// Control: an imported abstract `(sub resource)` passed by its
/// own index (a `Sub`-bounded generic, not a face) stays accepted.
#[test]
fn abstract_sub_generic_satisfies_resource_import() {
    expect_valid(
        r#"
(component
  (import "r" (type $r (sub resource)))
  (component $inner
    (import "r" (type $ri (sub resource))))
  (instance (instantiate $inner (with "r" (type $r)))))
"#,
    );
}

/// The same family in `GenericBound::assert_subtype_of`: the
/// `(Eq, Sub)` arm judged the bound target with `is_resource()`,
/// so an equality bound over a generic -- an abstract resource, a
/// `(sub resource)` face, or a plain export's `(eq)` face of a
/// resource -- was rejected against a `(sub resource)` bound even
/// though the bound denotes a resource. The shapes below route the
/// bound comparison through a component-type ascription.

#[test]
fn eq_bound_over_concrete_resource_matches_sub_bound() {
    expect_valid(
        r#"
(component
  (component $C
    (type $r (resource (rep i32)))
    (type $parent (component (import "r" (type (sub resource)))))
    (type $child (component (import "r" (type (eq $r)))))
    (export "c" (type $child) (type (eq $parent)))))
"#,
    );
}

#[test]
fn eq_bound_over_imported_sub_generic_matches_sub_bound() {
    expect_valid(
        r#"
(component
  (component $C
    (import "g" (type $g (sub resource)))
    (type $parent (component (import "r" (type (sub resource)))))
    (type $child (component (import "r" (type (eq $g)))))
    (export "c" (type $child) (type (eq $parent)))))
"#,
    );
}

#[test]
fn eq_bound_over_plain_export_face_matches_sub_bound() {
    expect_valid(
        r#"
(component
  (component $C
    (type $r (resource (rep i32)))
    (export $a "a" (type $r))
    (type $parent (component (import "r" (type (sub resource)))))
    (type $child (component (import "r" (type (eq $a)))))
    (export "c" (type $child) (type (eq $parent)))))
"#,
    );
}

#[test]
fn eq_bound_over_sub_ascribed_face_matches_sub_bound() {
    expect_valid(
        r#"
(component
  (component $C
    (type $r (resource (rep i32)))
    (export $a "a" (type $r) (type (sub resource)))
    (type $parent (component (import "r" (type (sub resource)))))
    (type $child (component (import "r" (type (eq $a)))))
    (export "c" (type $child) (type (eq $parent)))))
"#,
    );
}

/// Control: an equality bound over a primitive still does not
/// match a `(sub resource)` bound.
#[test]
fn eq_bound_over_primitive_does_not_match_sub_bound() {
    expect_invalid_with(
        r#"
(component
  (component $C
    (type $t u32)
    (type $parent (component (import "r" (type (sub resource)))))
    (type $child (component (import "r" (type (eq $t)))))
    (export "c" (type $child) (type (eq $parent)))))
"#,
        "expected any resource",
    );
}
