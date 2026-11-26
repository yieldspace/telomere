//! Regression tests for the second PR #384 Codex review round: the
//! validator traversals and the remaining identity/comparison
//! consumers that still treated a plain type export's `(eq)` face as
//! an opaque generic instead of resolving it to the type it denotes.
//!
//! - `decoder/validator/size.rs::type_contains_borrow_with_tracker`
//!   returned `false` for every generic, so a function result written
//!   through the face of a `(borrow …)` type slipped past the
//!   no-borrow-in-result prohibition;
//! - `decoder/validator/size.rs::type_id_needs_memory` treated the
//!   face as not needing memory, so a canon lift of a faced `string`
//!   type compiled without the required `memory` (and `realloc`)
//!   canonical options and failed only when called;
//! - the mixed `ValType::Type` / `ValType::Primitive` arm of
//!   `runtime/canonical.rs::val_types_equal_modulo_variant_form`
//!   compared the raw face, so a callable whose declared variant was
//!   ascribed as an `option` over a faced primitive was not presented
//!   under the export type and typed calls failed with
//!   `expected option result, got Variant`;
//! - `decoder/types/interface/annotated.rs::ensure_same_resource`
//!   normalised roots through plain-export faces only, so a name
//!   denoting a plain re-export of a `(sub resource)` face never
//!   reached the underlying resource and a matching `[constructor]`
//!   was rejected;
//! - the `(eq …)` ascription branch in `decoder/export.rs` compared
//!   types directly instead of through the ID-aware subtype check,
//!   so asserting that a plain-export face equals itself was rejected
//!   with `sub resource cannot assign to except sub resource`.

use telomere_component::{ComponentEngine, ComponentError, ComponentLinker, ComponentValue};

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

async fn instantiate_component(
    text: &str,
    linker: &ComponentLinker,
) -> (telomere::Store, telomere_component::ComponentInstance) {
    let bytes = wat::parse_str(text).expect("component wat must be valid");
    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");
    let store = telomere::Store::new();
    let instance = engine
        .instantiate(&program, &store, linker)
        .await
        .expect("instantiate should succeed");
    (store, instance)
}

// ---------------------------------------------------------------------
// Borrow containment: results must not contain borrows, through a face
// ---------------------------------------------------------------------

#[test]
fn borrow_result_through_plain_export_face_is_invalid() {
    expect_invalid_with(
        r#"
(component
  (import "r" (type $r (sub resource)))
  (type $b (borrow $r))
  (export $b' "b" (type $b))
  (core module $m
    (func (export "f") (result i32) (i32.const 0)))
  (core instance $i (instantiate $m))
  (func (export "f") (result $b')
    (canon lift (core func $i "f"))))
"#,
        "function result cannot contain a `borrow` type",
    );
}

#[test]
fn borrow_result_nested_in_list_through_plain_export_face_is_invalid() {
    expect_invalid_with(
        r#"
(component
  (import "r" (type $r (sub resource)))
  (type $l (list (borrow $r)))
  (export $l' "l" (type $l))
  (core module $m
    (func (export "f") (result i32) (i32.const 0)))
  (core instance $i (instantiate $m))
  (func (export "f") (result $l')
    (canon lift (core func $i "f"))))
"#,
        "function result cannot contain a `borrow` type",
    );
}

#[test]
fn borrow_param_through_plain_export_face_is_valid() {
    // Control: borrows are fine in parameter position, through a face
    // or directly.
    expect_valid(
        r#"
(component
  (import "r" (type $r (sub resource)))
  (type $b (borrow $r))
  (export $b' "b" (type $b))
  (core module $m
    (func (export "f") (param i32)))
  (core instance $i (instantiate $m))
  (func (export "f") (param "x" $b')
    (canon lift (core func $i "f"))))
"#,
    );
}

// ---------------------------------------------------------------------
// Memory requirement: canon options judge the denoted type
// ---------------------------------------------------------------------

#[test]
fn lift_faced_string_param_without_memory_is_invalid() {
    expect_invalid_with(
        r#"
(component
  (type $s string)
  (export $s' "s" (type $s))
  (core module $m
    (memory (export "memory") 1)
    (func (export "f") (param i32 i32) (result i32) (i32.const 7)))
  (core instance $i (instantiate $m))
  (func (export "f") (param "x" $s') (result u32)
    (canon lift (core func $i "f"))))
"#,
        "canonical option `memory` is required",
    );
}

#[test]
fn lift_faced_string_param_with_memory_and_realloc_is_valid() {
    // Control: with the required options present, the same lift is
    // accepted.
    expect_valid(
        r#"
(component
  (type $s string)
  (export $s' "s" (type $s))
  (core module $m
    (memory (export "memory") 1)
    (func (export "realloc") (param i32 i32 i32 i32) (result i32) (i32.const 0))
    (func (export "f") (param i32 i32) (result i32) (i32.const 7)))
  (core instance $i (instantiate $m))
  (func (export "f") (param "x" $s') (result u32)
    (canon lift (core func $i "f")
      (memory (core memory $i "memory"))
      (realloc (core func $i "realloc")))))
"#,
    );
}

#[test]
fn lift_faced_string_result_without_memory_is_invalid() {
    // Control: this one is rejected either way -- a string result
    // flattens to two words, which independently makes the result
    // indirect and requires `memory`. The discriminating shapes for
    // the memory traversal are the parameter ones above (a string
    // parameter flattens within the flat-parameter limit, so only
    // `type_needs_memory` can require the option).
    expect_invalid_with(
        r#"
(component
  (type $s string)
  (export $s' "s" (type $s))
  (core module $m
    (memory (export "memory") 1)
    (func (export "g") (result i32 i32) (i32.const 0) (i32.const 0)))
  (core instance $i (instantiate $m))
  (func (export "g") (result $s')
    (canon lift (core func $i "g"))))
"#,
        "canonical option `memory` is required",
    );
}

#[test]
fn lift_faced_record_with_string_field_without_memory_is_invalid() {
    // The face denotes a record whose field is a string: the memory
    // traversal must resolve the face and then keep walking.
    expect_invalid_with(
        r#"
(component
  (type $rec (record (field "x" string)))
  (export $rec' "rec" (type $rec))
  (core module $m
    (memory (export "memory") 1)
    (func (export "f") (param i32 i32) (result i32) (i32.const 7)))
  (core instance $i (instantiate $m))
  (func (export "f") (param "x" $rec') (result u32)
    (canon lift (core func $i "f"))))
"#,
        "canonical option `memory` is required",
    );
}

// ---------------------------------------------------------------------
// Mixed primitive comparison: presentation under an export ascription
// ---------------------------------------------------------------------

fn faced_option_param_component(ascription_payload: &str) -> String {
    format!(
        r#"
(component
  (type $u u32)
  (export $u' "u" (type $u))
  (type $v (variant (case "none") (case "some" u32)))
  (core module $m
    (func (export "f") (param i32 i32) (result i32) (local.get 1)))
  (core instance $i (instantiate $m))
  (func $f (param "x" $v) (result u32)
    (canon lift (core func $i "f")))
  (export "f" (func $f) (func (param "x" (option {ascription_payload})) (result u32))))
"#
    )
}

#[tokio::test]
async fn typed_option_param_honors_ascription_with_faced_payload() {
    // Flat path: the declared variant is a parameter, ascribed as an
    // `option` whose payload is written through the plain-export face.
    let text = faced_option_param_component("$u'");
    let (store, instance) = instantiate_component(&text, &ComponentLinker::new()).await;
    let func = instance
        .get_typed_func::<(Option<u32>,), u32>("f")
        .expect("ascribed option type must bind as Option");
    assert_eq!(
        func.call(&store, (Some(42),))
            .await
            .expect("typed call should succeed"),
        42
    );
    let dynamic = instance
        .call(
            &store,
            "f",
            &[ComponentValue::Option(Some(Box::new(ComponentValue::U32(
                42,
            ))))],
        )
        .await
        .expect("dynamic call should succeed");
    assert_eq!(dynamic, vec![ComponentValue::U32(42)]);
}

#[tokio::test]
async fn typed_option_param_honors_ascription_with_direct_payload() {
    // Control: the same ascription written with the primitive directly
    // (no face in the payload) works either way.
    let text = faced_option_param_component("u32");
    let (store, instance) = instantiate_component(&text, &ComponentLinker::new()).await;
    let func = instance
        .get_typed_func::<(Option<u32>,), u32>("f")
        .expect("ascribed option type must bind as Option");
    assert_eq!(
        func.call(&store, (Some(42),))
            .await
            .expect("typed call should succeed"),
        42
    );
}

/// The Codex-measured shape: two functions returning an option-shaped
/// declared variant through memory, ascribed as returning an `option`
/// whose payload is written through the plain-export face. `some`
/// writes discriminant 1 / payload 42 at offset 0; `none` points at
/// zeroed memory (discriminant 0).
fn faced_option_result_component() -> &'static str {
    r#"
(component
  (type $u u32)
  (export $u' "u" (type $u))
  (type $v (variant (case "none") (case "some" u32)))
  (core module $m
    (memory (export "memory") 1)
    (func (export "realloc") (param i32 i32 i32 i32) (result i32)
      i32.const 1024)
    (func (export "some") (result i32)
      i32.const 0
      i32.const 1
      i32.store
      i32.const 4
      i32.const 42
      i32.store
      i32.const 0)
    (func (export "none") (result i32)
      i32.const 64)
  )
  (core instance $i (instantiate $m))
  (func $some (result $v)
    (canon lift (core func $i "some") (memory (core memory $i "memory")) (realloc (core func $i "realloc"))))
  (func $none (result $v)
    (canon lift (core func $i "none") (memory (core memory $i "memory")) (realloc (core func $i "realloc"))))
  (export "some" (func $some) (func (result (option $u'))))
  (export "none" (func $none) (func (result (option $u'))))
)
"#
}

#[tokio::test]
async fn typed_option_result_honors_ascription_with_faced_payload() {
    let (store, instance) =
        instantiate_component(faced_option_result_component(), &ComponentLinker::new()).await;
    let some = instance
        .get_typed_func::<(), Option<u32>>("some")
        .expect("ascribed option type must bind as Option");
    assert_eq!(
        some.call(&store, ())
            .await
            .expect("typed call should succeed"),
        Some(42)
    );
    let none = instance
        .get_typed_func::<(), Option<u32>>("none")
        .expect("ascribed option type must bind as Option");
    assert_eq!(
        none.call(&store, ())
            .await
            .expect("typed call should succeed"),
        None
    );
    let dynamic = instance
        .call(&store, "some", &[])
        .await
        .expect("dynamic call should succeed");
    assert_eq!(
        dynamic,
        vec![ComponentValue::Option(Some(Box::new(ComponentValue::U32(
            42
        ))))]
    );
}

// ---------------------------------------------------------------------
// Annotated names: roots resolve through sub-ascribed faces too
// ---------------------------------------------------------------------

#[test]
fn constructor_for_plainly_reexported_sub_face_is_valid() {
    // The name "r" denotes a plain re-export of a `(sub resource)`
    // face; its identity root is the underlying resource, so a
    // constructor returning `(own $r)` constructs "r".
    expect_valid(
        r#"
(component
  (type $r (resource (rep i32)))
  (export $face "s" (type $r) (type (sub resource)))
  (export "r" (type $face))
  (core module $m
    (func (export "mk") (result i32) unreachable))
  (core instance $i (instantiate $m))
  (func $ctor (result (own $r)) (canon lift (core func $i "mk")))
  (export "[constructor]r" (func $ctor)))
"#,
    );
}

#[test]
fn constructor_result_through_sub_face_for_reexported_name_is_valid() {
    // Control: writing the constructor's result through the sub face
    // itself names the same resource.
    expect_valid(
        r#"
(component
  (type $r (resource (rep i32)))
  (export $face "s" (type $r) (type (sub resource)))
  (export "r" (type $face))
  (core module $m
    (func (export "mk") (result i32) unreachable))
  (core instance $i (instantiate $m))
  (func $ctor (result (own $face)) (canon lift (core func $i "mk")))
  (export "[constructor]r" (func $ctor)))
"#,
    );
}

#[test]
fn constructor_for_other_resource_under_reexported_name_is_invalid() {
    // Control: a constructor of a different resource does not become
    // the constructor of "r" by going through the face chain.
    expect_invalid_with(
        r#"
(component
  (type $r (resource (rep i32)))
  (type $q (resource (rep i32)))
  (export $face "s" (type $r) (type (sub resource)))
  (export "r" (type $face))
  (core module $m
    (func (export "mk") (result i32) unreachable))
  (core instance $i (instantiate $m))
  (func $ctor (result (own $q)) (canon lift (core func $i "mk")))
  (export "[constructor]r" (func $ctor)))
"#,
        "resource used in function does not have a name in this context",
    );
}

// ---------------------------------------------------------------------
// (eq …) ascriptions over plain-export faces
// ---------------------------------------------------------------------

#[test]
fn eq_ascription_asserting_plain_face_equals_itself_is_valid() {
    expect_valid(
        r#"
(component
  (import "r" (type $r (sub resource)))
  (export $a "a" (type $r))
  (export "b" (type $a) (type (eq $a))))
"#,
    );
}

#[test]
fn eq_ascription_to_other_resources_face_is_invalid() {
    // Control: an `(eq …)` assertion between faces of two different
    // resources is still rejected, by the resource-identity check.
    expect_invalid_with(
        r#"
(component
  (import "r" (type $r (sub resource)))
  (import "q" (type $q (sub resource)))
  (export $a "a" (type $r))
  (export $b "b" (type $q))
  (export "c" (type $a) (type (eq $b))))
"#,
        "resource types are not the same",
    );
}
