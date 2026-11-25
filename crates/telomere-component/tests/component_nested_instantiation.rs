//! Regression tests for #316: instantiation arguments supplied with
//! `(with ...)` when instantiating a nested component must be honoured by
//! the inner component's core instances. Previously `instantiate_sync`
//! materialised every core instance in the flattened program with the root
//! environment, so any inner core instance depending on an instantiation
//! argument failed to link.

use telomere_component::{ComponentEngine, ComponentLinker, ComponentValue};

fn compile_component(text: &str) -> Vec<u8> {
    wat::parse_str(text).expect("component wat must be valid")
}

async fn call_g(wat: &str) -> Vec<ComponentValue> {
    let bytes = compile_component(wat);
    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");
    let store = telomere::Store::new();
    let linker = ComponentLinker::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");
    instance
        .call(&store, "g", &[])
        .await
        .expect("call should succeed")
}

/// Form A: a func argument consumed by the inner component's core instance
/// through a canon lower.
#[tokio::test]
async fn nested_instantiation_func_argument_reaches_core_instance() {
    let result = call_g(
        r#"
(component
  (component $C
    (core module $m (func (export "f") (result i32) i32.const 42))
    (core instance $i (instantiate $m))
    (func (export "f") (result u32) (canon lift (core func $i "f"))))
  (instance $c (instantiate $C))
  (component $D
    (import "f" (func $f (result u32)))
    (core func $lowered (canon lower (func $f)))
    (core module $m
      (import "" "f" (func (result i32)))
      (func (export "g") (result i32) (call 0)))
    (core instance $i (instantiate $m (with "" (instance (export "f" (func $lowered))))))
    (func (export "g") (result u32) (canon lift (core func $i "g"))))
  (instance $d (instantiate $D (with "f" (func $c "f"))))
  (export "g" (func $d "g")))
"#,
    )
    .await;
    assert_eq!(result, vec![ComponentValue::U32(42)]);
}

/// Form B: an instance argument whose exported func is lowered into the
/// inner component's core instance.
#[tokio::test]
async fn nested_instantiation_instance_argument_reaches_core_instance() {
    let result = call_g(
        r#"
(component
  (component $Provider
    (core module $m (func (export "f") (result i32) i32.const 7))
    (core instance $i (instantiate $m))
    (func (export "f") (result u32) (canon lift (core func $i "f"))))
  (instance $p (instantiate $Provider))
  (component $D
    (import "svc" (instance $svc (export "f" (func (result u32)))))
    (core func $lowered (canon lower (func $svc "f")))
    (core module $m
      (import "" "f" (func (result i32)))
      (func (export "g") (result i32) (call 0)))
    (core instance $i (instantiate $m (with "" (instance (export "f" (func $lowered))))))
    (func (export "g") (result u32) (canon lift (core func $i "g"))))
  (instance $d (instantiate $D (with "svc" (instance $p))))
  (export "g" (func $d "g")))
"#,
    )
    .await;
    assert_eq!(result, vec![ComponentValue::U32(7)]);
}

/// Form C: a core module argument instantiated by the inner component.
#[tokio::test]
async fn nested_instantiation_core_module_argument_is_instantiated() {
    let result = call_g(
        r#"
(component
  (core module $Lib (func (export "f") (result i32) i32.const 99))
  (component $D
    (import "libc" (core module $LibTy (export "f" (func (result i32)))))
    (core instance $i (instantiate $LibTy))
    (func (export "g") (result u32) (canon lift (core func $i "f"))))
  (instance $d (instantiate $D (with "libc" (core module $Lib))))
  (export "g" (func $d "g")))
"#,
    )
    .await;
    assert_eq!(result, vec![ComponentValue::U32(99)]);
}
