//! Regression tests for the PR #384 Codex review: consumers that
//! inspect a type's shape or kind must resolve a plain type export's
//! `(eq)` face to the type it denotes before judging.
//!
//! A plain type export mints an `(eq)` face in the type index space
//! (#365), so any index written through the export denotes the face,
//! not the underlying type. Three consumer classes still read the raw
//! face on the reviewed head:
//!
//! - typed bindings (`func/typecheck.rs::type_matches`) rejected a
//!   primitive reached through the face, so a `u32` export used as a
//!   canon-lifted function's param/result could be called dynamically
//!   but not through `get_typed_func`;
//! - the annotated-name shape helpers
//!   (`decoder/types/interface/annotated.rs`) required a direct
//!   `DefVal(Own/Borrow/Variant)`, so a constructor/method written
//!   through a plain-exported `own`/`borrow`/result alias failed with
//!   `should return (own $T)` / `should take a first argument of
//!   (borrow $T)`;
//! - the canon resource-kind check (`decoder/canon/parse.rs`) and the
//!   `own`/`borrow` operand check (`decoder/types/mod.rs`) accepted
//!   every generic, so a face denoting a primitive passed as a
//!   resource: `resource.drop` on an exported `u32`, and even
//!   `(own <exported u32>)`, compiled;
//! - the value index space (`decoder/value.rs`, and the host-value
//!   check in `runtime/env.rs`) required a direct `DefVal`, so an
//!   `(eq)` type -- an imported alias, or a face used by a binary
//!   value definition -- was rejected as "does not resolve to a
//!   defined value type" even though it denotes one.

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
// Typed bindings: primitives through a plain-export face
// ---------------------------------------------------------------------

const PRIMITIVE_FACE_COMPONENT: &str = r#"
(component
  (type $u u32)
  (export $u' "u" (type $u))
  (core module $m
    (func (export "echo") (param i32) (result i32) (local.get 0))
    (func (export "answer") (result i32) (i32.const 42)))
  (core instance $i (instantiate $m))
  (func (export "echo") (param "x" $u') (result $u')
    (canon lift (core func $i "echo")))
  (func (export "answer") (result $u')
    (canon lift (core func $i "answer"))))
"#;

#[tokio::test]
async fn typed_binding_accepts_primitive_result_through_plain_export_face() {
    let (store, instance) =
        instantiate_component(PRIMITIVE_FACE_COMPONENT, &ComponentLinker::new()).await;
    let func = instance
        .get_typed_func::<(), u32>("answer")
        .expect("a primitive result denoted through the export face must bind as u32");
    assert_eq!(
        func.call(&store, ()).await.expect("typed call succeeds"),
        42
    );
    // The dynamic API already worked: it returns the same value.
    let dynamic = instance
        .call(&store, "answer", &[])
        .await
        .expect("dynamic call should succeed");
    assert_eq!(dynamic, vec![ComponentValue::U32(42)]);
}

#[tokio::test]
async fn typed_binding_accepts_primitive_param_through_plain_export_face() {
    let (store, instance) =
        instantiate_component(PRIMITIVE_FACE_COMPONENT, &ComponentLinker::new()).await;
    let func = instance
        .get_typed_func::<(u32,), u32>("echo")
        .expect("a primitive param denoted through the export face must bind as u32");
    assert_eq!(
        func.call(&store, (7,)).await.expect("typed call succeeds"),
        7
    );
    let dynamic = instance
        .call(&store, "echo", &[ComponentValue::U32(9)])
        .await
        .expect("dynamic call should succeed");
    assert_eq!(dynamic, vec![ComponentValue::U32(9)]);
}

#[tokio::test]
async fn typed_binding_accepts_option_payload_through_plain_export_face() {
    // The primitive is nested inside a container: the option's payload
    // type is written through the export face.
    let component = r#"
(component
  (type $u u32)
  (export $u' "u" (type $u))
  (type $o (option $u'))
  (core module $m
    (memory (export "memory") 1)
    (func (export "realloc") (param i32 i32 i32 i32) (result i32) (i32.const 0))
    (func (export "f") (result i32)
      (i32.store8 (i32.const 0) (i32.const 1))
      (i32.store (i32.const 4) (i32.const 42))
      (i32.const 0)))
  (core instance $i (instantiate $m))
  (func (export "f") (result $o)
    (canon lift (core func $i "f") (memory (core memory $i "memory")) (realloc (core func $i "realloc")))))
"#;
    let (store, instance) = instantiate_component(component, &ComponentLinker::new()).await;
    let func = instance
        .get_typed_func::<(), Option<u32>>("f")
        .expect("an option payload denoted through the export face must bind as u32");
    assert_eq!(
        func.call(&store, ()).await.expect("typed call succeeds"),
        Some(42)
    );
}

// ---------------------------------------------------------------------
// Annotated names: own/borrow/result aliases through a plain-export face
// ---------------------------------------------------------------------

#[test]
fn constructor_result_through_plain_export_own_face_is_valid() {
    // The constructor's result is written through the plain export of
    // the `own` alias: the alias's face denotes exactly the own type,
    // so this constructs the re-exported resource "r".
    expect_valid(
        r#"
(component
  (import "r" (type $R (sub resource)))
  (export "r" (type $R))
  (type $O (own $R))
  (export $O' "o" (type $O))
  (core module $m
    (func (export "mk") (result i32) unreachable))
  (core instance $i (instantiate $m))
  (func $ctor (result $O') (canon lift (core func $i "mk")))
  (export "[constructor]r" (func $ctor)))
"#,
    );
}

#[test]
fn method_param_through_plain_export_borrow_face_is_valid() {
    expect_valid(
        r#"
(component
  (import "r" (type $R (sub resource)))
  (export "r" (type $R))
  (type $B (borrow $R))
  (export $B' "b" (type $B))
  (core module $m
    (func (export "use") (param i32)))
  (core instance $i (instantiate $m))
  (func $meth (param "self" $B') (canon lift (core func $i "use")))
  (export "[method]r.f" (func $meth)))
"#,
    );
}

#[test]
fn constructor_result_through_plain_export_result_face_is_valid() {
    // The `(result (own $T))` form written through the plain export of
    // the result alias must be recognised as a constructor result too.
    expect_valid(
        r#"
(component
  (import "r" (type $R (sub resource)))
  (export "r" (type $R))
  (type $Res (result (own $R)))
  (export $Res' "res" (type $Res))
  (core module $m
    (memory (export "memory") 1)
    (func (export "realloc") (param i32 i32 i32 i32) (result i32) (i32.const 0))
    (func (export "mk") (result i32) (i32.const 0)))
  (core instance $i (instantiate $m))
  (func $ctor (result $Res')
    (canon lift (core func $i "mk") (memory (core memory $i "memory")) (realloc (core func $i "realloc"))))
  (export "[constructor]r" (func $ctor)))
"#,
    );
}

// ---------------------------------------------------------------------
// Canon resource ops: the kind check judges the denoted type
// ---------------------------------------------------------------------

#[test]
fn resource_drop_on_plain_export_face_of_primitive_is_invalid() {
    expect_invalid_with(
        r#"
(component
  (type $t u32)
  (export $t2 "t" (type $t))
  (core func (canon resource.drop $t2)))
"#,
        "not a resource type",
    );
}

#[test]
fn resource_new_on_plain_export_face_of_primitive_is_invalid() {
    expect_invalid_with(
        r#"
(component
  (type $t u32)
  (export $t2 "t" (type $t))
  (core func (canon resource.new $t2)))
"#,
        "not a resource type",
    );
}

#[test]
fn resource_rep_on_plain_export_face_of_primitive_is_invalid() {
    expect_invalid_with(
        r#"
(component
  (type $t u32)
  (export $t2 "t" (type $t))
  (core func (canon resource.rep $t2)))
"#,
        "not a resource type",
    );
}

#[test]
fn resource_drop_on_plain_export_face_of_resource_is_valid() {
    // Control: a face that denotes a resource stays a resource.
    expect_valid(
        r#"
(component
  (type $r (resource (rep i32)))
  (export $r' "r" (type $r))
  (core func (canon resource.drop $r')))
"#,
    );
}

#[test]
fn resource_drop_on_abstract_resource_is_valid() {
    // Control: a `(sub resource)` generic denotes no concrete type but
    // is a resource type in its own right.
    expect_valid(
        r#"
(component
  (import "r" (type $R (sub resource)))
  (core func (canon resource.drop $R)))
"#,
    );
}

// ---------------------------------------------------------------------
// own/borrow operands: the kind check judges the denoted type
// ---------------------------------------------------------------------

#[test]
fn own_of_plain_export_face_of_primitive_is_invalid() {
    expect_invalid_with(
        r#"
(component
  (type $t u32)
  (export $t2 "t" (type $t))
  (type $o (own $t2)))
"#,
        "not a resource type",
    );
}

#[test]
fn borrow_of_plain_export_face_of_primitive_is_invalid() {
    expect_invalid_with(
        r#"
(component
  (type $t u32)
  (export $t2 "t" (type $t))
  (type $b (borrow $t2)))
"#,
        "not a resource type",
    );
}

#[test]
fn own_of_plain_export_face_of_resource_is_valid() {
    // Control: a face that denotes a resource is a valid own operand.
    expect_valid(
        r#"
(component
  (import "r" (type $R (sub resource)))
  (export $R' "r" (type $R))
  (type $o (own $R')))
"#,
    );
}

// ---------------------------------------------------------------------
// Value index space: `(eq)` types resolve to the type they denote
// ---------------------------------------------------------------------
//
// A value *import* cannot be typed through a plain-export face (the
// visibility rules forbid imports from depending on exports' names),
// and an imported `(eq)` alias registers the denoted type directly,
// so neither reaches the value-space checks as a generic. A binary
// value *definition* can name the face, though: its entry's type is
// parsed after the export section has minted the face, and it was
// rejected with "value type does not resolve to a defined value
// type" even though the face denotes a defined value type.

#[tokio::test]
async fn value_import_typed_through_eq_alias_matches() {
    let component = r#"
(component
  (type $u u32)
  (import "t" (type $t (eq $u)))
  (import "v" (value $v (type $t)))
  (export "v" (value $v)))
"#;
    let mut linker = ComponentLinker::new();
    linker.register_import_value("v", ComponentValue::U32(42));
    let (store, instance) = instantiate_component(component, &linker).await;
    assert_eq!(
        instance.get_value(&store, "v").expect("value resolves"),
        ComponentValue::U32(42)
    );
}

#[tokio::test]
async fn defined_value_typed_through_plain_export_face_decodes() {
    // Hand-built binary: the value section's entry is typed by the
    // face index a plain type export mints, which no text-format
    // definition can name (definitions use the value section, whose
    // entries are not text-expressible).
    fn uleb(mut value: u64) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let mut byte = (value & 0x7f) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            out.push(byte);
            if value == 0 {
                return out;
            }
        }
    }
    fn name(s: &str) -> Vec<u8> {
        let mut out = uleb(s.len() as u64);
        out.extend_from_slice(s.as_bytes());
        out
    }
    fn dash_name(s: &str) -> Vec<u8> {
        let mut out = vec![0x00];
        out.extend(name(s));
        out
    }
    fn section(id: u8, payload: &[u8]) -> Vec<u8> {
        let mut out = vec![id];
        out.extend(uleb(payload.len() as u64));
        out.extend_from_slice(payload);
        out
    }
    fn vec_of(items: &[Vec<u8>]) -> Vec<u8> {
        let mut out = uleb(items.len() as u64);
        for item in items {
            out.extend_from_slice(item);
        }
        out
    }
    // Type section: type 0 is `u32` (defvaltype opcode 0x79).
    let types = section(0x07, &vec_of(&[vec![0x79]]));
    // Export section: plain export of type 0 as "u" (sort 0x03, no
    // desc). Processing it mints the `(eq)` face as type index 1.
    let mut export_type = dash_name("u");
    export_type.push(0x03);
    export_type.extend(uleb(0));
    export_type.push(0x00);
    let type_exports = section(0x0b, &vec_of(&[export_type]));
    // Value section: one entry typed by type index 1 (the face),
    // encoding the u32 value 42.
    let mut entry = vec![0x01];
    entry.extend(uleb(1));
    entry.extend(uleb(42));
    let values = section(0x0c, &vec_of(&[entry]));
    // Export section: the defined value (sort 0x02, index 0, no desc).
    let mut export_value = dash_name("v");
    export_value.push(0x02);
    export_value.extend(uleb(0));
    export_value.push(0x00);
    let value_exports = section(0x0b, &vec_of(&[export_value]));

    let mut bytes = b"\x00asm".to_vec();
    bytes.extend([0x0d, 0x00, 0x01, 0x00]);
    for s in [&types, &type_exports, &values, &value_exports] {
        bytes.extend_from_slice(s);
    }

    let engine = ComponentEngine::new();
    let program = engine
        .compile(&bytes)
        .expect("a value defined through the export face must compile");
    let store = telomere::Store::new();
    let instance = engine
        .instantiate(&program, &store, &ComponentLinker::new())
        .await
        .expect("instantiate should succeed");
    assert_eq!(
        instance.get_value(&store, "v").expect("value resolves"),
        ComponentValue::U32(42)
    );
}
