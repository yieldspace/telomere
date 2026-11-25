//! Regression tests for #313: every import a component declares must be
//! satisfied at instantiation time, whether or not anything in the
//! component references it. Before the fix, an import that was never used
//! (not referenced by any alias / lower / export) was never resolved, so a
//! component with an unsatisfiable import instantiated successfully.

use telomere_component::{ComponentEngine, ComponentLinker};

fn compile_component(text: &str) -> Vec<u8> {
    wat::parse_str(text).expect("component wat must be valid")
}

async fn instantiate(text: &str) -> Result<(), telomere_component::ComponentError> {
    let bytes = compile_component(text);
    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");
    let store = telomere::Store::new();
    let linker = ComponentLinker::new();
    engine
        .instantiate(&program, &store, &linker)
        .await
        .map(|_| ())
}

/// A component whose only content besides the import is an unrelated
/// exported core function, so instantiation has real work to do and the
/// import is provably unreferenced.
const WITH_UNRELATED_EXPORT: &str = r#"
(component
  (core module $m
    (func (export "f") (result i32) i32.const 7))
  (core instance $i (instantiate $m))
  (func (export "answer") (result u32)
    (canon lift (core func $i "f")))
  IMPORT
)
"#;

#[tokio::test]
async fn unused_named_func_import_fails_instantiation() {
    let text = WITH_UNRELATED_EXPORT.replace(
        "IMPORT",
        r#"(import "missing" (func $missing (param "x" s32) (result s32)))"#,
    );
    let err = instantiate(&text)
        .await
        .expect_err("unused func import must fail instantiation");
    assert!(
        err.to_string()
            .contains("function import 'missing' is unresolved"),
        "unexpected error: {err}"
    );
}

#[tokio::test]
async fn unused_anonymous_func_import_fails_instantiation() {
    let text = WITH_UNRELATED_EXPORT.replace(
        "IMPORT",
        r#"(import "missing" (func (param "x" s32) (result s32)))"#,
    );
    let err = instantiate(&text)
        .await
        .expect_err("unused func import must fail instantiation");
    assert!(
        err.to_string()
            .contains("function import 'missing' is unresolved"),
        "unexpected error: {err}"
    );
}

#[tokio::test]
async fn unused_instance_import_fails_instantiation() {
    let text = WITH_UNRELATED_EXPORT.replace(
        "IMPORT",
        r#"(import "missing" (instance (export "run" (func))))"#,
    );
    let err = instantiate(&text)
        .await
        .expect_err("unused instance import must fail instantiation");
    assert!(
        err.to_string().contains("missing"),
        "error must name the missing import: {err}"
    );
}

#[tokio::test]
async fn unused_value_import_fails_instantiation() {
    let text = WITH_UNRELATED_EXPORT.replace("IMPORT", r#"(import "missing" (value u32))"#);
    let err = instantiate(&text)
        .await
        .expect_err("unused value import must fail instantiation");
    assert!(
        err.to_string()
            .contains("value import 'missing' is unresolved"),
        "unexpected error: {err}"
    );
}

#[tokio::test]
async fn unused_core_module_import_fails_instantiation() {
    let text = WITH_UNRELATED_EXPORT.replace(
        "IMPORT",
        r#"(import "missing" (core module (export "mem" (memory 1))))"#,
    );
    let err = instantiate(&text)
        .await
        .expect_err("unused core module import must fail instantiation");
    assert!(
        err.to_string().contains("missing"),
        "error must name the missing import: {err}"
    );
}

#[tokio::test]
async fn unused_component_import_fails_instantiation() {
    let text = r#"
(component
  (type $ct (component))
  (import "missing" (component (type $ct)))
  (core module $m
    (func (export "f") (result i32) i32.const 7))
  (core instance $i (instantiate $m))
  (func (export "answer") (result u32)
    (canon lift (core func $i "f")))
)
"#;
    let err = instantiate(text)
        .await
        .expect_err("unused component import must fail instantiation");
    assert!(
        err.to_string().contains("missing"),
        "error must name the missing import: {err}"
    );
}

/// Control: the same shape with the import satisfied by the linker must
/// still instantiate and run, so the check cannot be a blanket rejection
/// of components that declare imports.
#[tokio::test]
async fn satisfied_but_unused_func_import_still_instantiates() {
    let bytes = compile_component(
        r#"
(component
  (type (func (param "x" s32) (result s32)))
  (import "host-f" (func (type 0)))
  (core module $m
    (func (export "f") (result i32) i32.const 7))
  (core instance $i (instantiate $m))
  (func (export "answer") (result u32)
    (canon lift (core func $i "f")))
)
"#,
    );
    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");
    let store = telomere::Store::new();
    let mut linker = ComponentLinker::new();
    linker.register_import("host-f", |_store, args| Ok(vec![args[0].clone()]));
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("satisfied import must instantiate");
    let result = instance
        .call(&store, "answer", &[])
        .await
        .expect("call should succeed");
    assert_eq!(result, vec![telomere_component::ComponentValue::U32(7)]);
}

/// End-to-end: a nested component whose import is fed from an outer
/// import that the linker does not satisfy must fail instantiation, even
/// though the import is never used inside the nested component. (An
/// instantiation that omits the argument entirely is rejected even
/// earlier, by the validator: `missing import named ...`.)
#[tokio::test]
async fn nested_instance_with_unsatisfied_unused_import_fails() {
    let text = r#"
(component
  (import "missing" (func (param "x" s32) (result s32)))
  (component $inner
    (import "missing" (func (param "x" s32) (result s32)))
    (core module $m
      (func (export "f") (result i32) i32.const 7))
    (core instance $i (instantiate $m))
    (func (export "answer") (result u32)
      (canon lift (core func $i "f"))))
  (instance $inst (instantiate $inner (with "missing" (func 0))))
  (export "answer" (func $inst "answer"))
)
"#;
    let err = instantiate(text)
        .await
        .expect_err("nested unsatisfied import must fail instantiation");
    assert!(
        err.to_string().contains("missing"),
        "error must name the missing import: {err}"
    );
}
