//! Regression tests for the third PR #384 Codex review round: the
//! `Own`/`Borrow` leaf of
//! `runtime/canonical.rs::defval_types_equal_modulo_variant_form`
//! read handle payloads with `program.get_type`, so a resource named
//! through a plain type export's `(eq)` face never compared equal to
//! the resource index a definition uses directly. A declared variant
//! over `(own $r)` whose export ascription rewrote it as an `option`
//! over `(own $face)` was therefore not presented under the export
//! type: the lifted result kept the declared `Variant` representation
//! instead of the ascribed `Option` one.
//!
//! The parameter direction of the same shapes already worked (the
//! argument conversion does not consult this comparison the same
//! way), and an ascription naming a *different* resource is rejected
//! at validation time; both are kept as controls.

use telomere_component::{ComponentEngine, ComponentLinker, ComponentValue};

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

/// A declared variant over `(own <declared>)`, returned through
/// memory, whose export ascription is written as
/// `(option (own <ascribed>))`. `none` points at zeroed memory
/// (discriminant 0), so the returned value's representation --
/// `Option(None)` under the ascription, `Variant { case: "none" }`
/// under the declared type -- is the observable. (The `some` case
/// needs a live handle, which a core function cannot mint; the
/// comparison this tests does not depend on the payload value.)
fn own_result_component(declared: &str, ascribed: &str) -> String {
    format!(
        r#"
(component
  (type $r (resource (rep i32)))
  (type $q (resource (rep i32)))
  (export $r' "r" (type $r))
  (export $q' "q" (type $q))
  (type $v (variant (case "none") (case "some" (own {declared}))))
  (core module $m
    (memory (export "memory") 1)
    (func (export "realloc") (param i32 i32 i32 i32) (result i32)
      i32.const 1024)
    (func (export "none") (result i32)
      i32.const 64))
  (core instance $i (instantiate $m))
  (func $none (result $v)
    (canon lift (core func $i "none") (memory (core memory $i "memory")) (realloc (core func $i "realloc"))))
  (export "none" (func $none) (func (result (option (own {ascribed}))))))
"#
    )
}

/// A declared variant parameter over `({kind} $r)` whose export
/// ascription is written as `(option ({kind} <ascribed>))`. The core
/// func echoes the discriminant, so `Option(None)` in gives 0 out.
fn param_component(kind: &str, ascribed: &str) -> String {
    format!(
        r#"
(component
  (type $r (resource (rep i32)))
  (type $q (resource (rep i32)))
  (export $r' "r" (type $r))
  (export $q' "q" (type $q))
  (type $v (variant (case "none") (case "some" ({kind} $r))))
  (core module $m
    (func (export "f") (param i32 i32) (result i32) (local.get 0)))
  (core instance $i (instantiate $m))
  (func $f (param "x" $v) (result u32)
    (canon lift (core func $i "f")))
  (export "f" (func $f) (func (param "x" (option ({kind} {ascribed}))) (result u32))))
"#
    )
}

// ---------------------------------------------------------------------
// Resource leaves: faces resolve in the presentation comparison
// ---------------------------------------------------------------------

#[tokio::test]
async fn own_result_ascribed_through_plain_export_face_presents_as_option() {
    // The finding's shape: the definition names the resource by its
    // index, the ascription can only name it through the plain-export
    // face (surface validation requires the exported name there).
    let text = own_result_component("$r", "$r'");
    let (store, instance) = instantiate_component(&text, &ComponentLinker::new()).await;
    let dynamic = instance
        .call(&store, "none", &[])
        .await
        .expect("dynamic call should succeed");
    assert_eq!(dynamic, vec![ComponentValue::Option(None)]);
}

#[tokio::test]
async fn own_result_ascribed_through_imported_resources_face_presents_as_option() {
    // The same shape over an imported abstract resource: the face
    // denotes the `(sub resource)` entry itself.
    let text = r#"
(component
  (import "r" (type $r (sub resource)))
  (export $r' "r" (type $r))
  (type $v (variant (case "none") (case "some" (own $r))))
  (core module $m
    (memory (export "memory") 1)
    (func (export "realloc") (param i32 i32 i32 i32) (result i32)
      i32.const 1024)
    (func (export "none") (result i32)
      i32.const 64))
  (core instance $i (instantiate $m))
  (func $none (result $v)
    (canon lift (core func $i "none") (memory (core memory $i "memory")) (realloc (core func $i "realloc"))))
  (export "none" (func $none) (func (result (option (own $r'))))))
"#;
    let (store, instance) = instantiate_component(text, &ComponentLinker::new()).await;
    let dynamic = instance
        .call(&store, "none", &[])
        .await
        .expect("dynamic call should succeed");
    assert_eq!(dynamic, vec![ComponentValue::Option(None)]);
}

#[tokio::test]
async fn own_result_ascribed_through_face_of_face_presents_as_option() {
    // The ascription names the resource through a plain export of a
    // plain export -- a chain of two `(eq)` faces.
    let text = r#"
(component
  (type $r (resource (rep i32)))
  (export $r' "r" (type $r))
  (export $r'' "r2" (type $r'))
  (type $v (variant (case "none") (case "some" (own $r))))
  (core module $m
    (memory (export "memory") 1)
    (func (export "realloc") (param i32 i32 i32 i32) (result i32)
      i32.const 1024)
    (func (export "none") (result i32)
      i32.const 64))
  (core instance $i (instantiate $m))
  (func $none (result $v)
    (canon lift (core func $i "none") (memory (core memory $i "memory")) (realloc (core func $i "realloc"))))
  (export "none" (func $none) (func (result (option (own $r''))))))
"#;
    let (store, instance) = instantiate_component(text, &ComponentLinker::new()).await;
    let dynamic = instance
        .call(&store, "none", &[])
        .await
        .expect("dynamic call should succeed");
    assert_eq!(dynamic, vec![ComponentValue::Option(None)]);
}

#[tokio::test]
async fn own_result_declared_and_ascribed_through_face_presents_as_option() {
    // Control: the declared variant also mentions the resource
    // through its plain-export face (the payload indices are then
    // the same face on both sides).
    let text = own_result_component("$r'", "$r'");
    let (store, instance) = instantiate_component(&text, &ComponentLinker::new()).await;
    let dynamic = instance
        .call(&store, "none", &[])
        .await
        .expect("dynamic call should succeed");
    assert_eq!(dynamic, vec![ComponentValue::Option(None)]);
}

// ---------------------------------------------------------------------
// Controls: parameter direction, and strictness for other resources
// ---------------------------------------------------------------------

#[tokio::test]
async fn own_param_ascribed_through_plain_export_face_is_accepted() {
    // Control: the parameter direction of the finding's shape.
    let text = param_component("own", "$r'");
    let (store, instance) = instantiate_component(&text, &ComponentLinker::new()).await;
    let dynamic = instance
        .call(&store, "f", &[ComponentValue::Option(None)])
        .await
        .expect("dynamic call should succeed");
    assert_eq!(dynamic, vec![ComponentValue::U32(0)]);
}

#[tokio::test]
async fn borrow_param_ascribed_through_plain_export_face_is_accepted() {
    // Control: the `Borrow` leaf of the same arm, parameter side.
    let text = param_component("borrow", "$r'");
    let (store, instance) = instantiate_component(&text, &ComponentLinker::new()).await;
    let dynamic = instance
        .call(&store, "f", &[ComponentValue::Option(None)])
        .await
        .expect("dynamic call should succeed");
    assert_eq!(dynamic, vec![ComponentValue::U32(0)]);
}

#[test]
fn own_result_ascribed_through_other_resources_face_is_invalid() {
    // Control (strictness): an ascription naming a *different*
    // resource through its face is rejected at validation time --
    // resolving faces must not blur distinct resources together.
    let text = own_result_component("$r", "$q'");
    expect_invalid_with(&text, "resource types are not the same");
}

#[test]
fn borrow_param_ascribed_through_other_resources_face_is_invalid() {
    // Control (strictness): the same, on the `Borrow` leaf.
    let text = param_component("borrow", "$q'");
    expect_invalid_with(&text, "resource types are not the same");
}
