mod common;

use common::instantiate_wat;
use telomere::component::{ComponentEngine, ComponentError, ComponentLinker, ComponentValue};
use telomere::Registry;

fn compile_component(text: &str) -> Vec<u8> {
    wat::parse_str(text).expect("component wat must be valid")
}

#[tokio::test]
async fn component_runtime_calls_async_import_as_lifted_export() {
    let bytes = compile_component(
        r#"
(component
  (type (func (param "lhs" s32) (param "rhs" s32) (result s32)))
  (import "host-add" (func (type 0)))
  (export "add" (func 0))
)
"#,
    );

    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");

    let mut linker = ComponentLinker::new();
    linker.register_import_async("host-add", |_store, args| {
        Box::pin(async move {
            if args.len() != 2 {
                return Err(ComponentError::InvalidArgument(
                    "host-add expects exactly 2 arguments".to_owned(),
                ));
            }
            let lhs = args[0]
                .as_i32()
                .ok_or_else(|| ComponentError::InvalidArgument("arg[0] must be i32".to_owned()))?;
            let rhs = args[1]
                .as_i32()
                .ok_or_else(|| ComponentError::InvalidArgument("arg[1] must be i32".to_owned()))?;
            Ok(vec![ComponentValue::I32(lhs + rhs)])
        })
    });

    let mut store = telomere::Store::new();
    let instance = engine
        .instantiate(&program, &mut store, &linker)
        .await
        .expect("instantiate should succeed");

    let result = instance
        .call(
            &mut store,
            "add",
            &[ComponentValue::I32(20), ComponentValue::I32(22)],
        )
        .await
        .expect("call should succeed");

    assert_eq!(result, vec![ComponentValue::I32(42)]);
}

#[tokio::test]
async fn component_runtime_calls_registered_core_export() {
    let bytes = compile_component(
        r#"
(component
  (type (func (param "lhs" s32) (param "rhs" s32) (result s32)))
  (import "core-add" (func (type 0)))
  (export "add" (func 0))
)
"#,
    );

    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");
    let mut store = telomere::Store::new();
    let registry = Registry::new();
    let core = instantiate_wat(
        r#"
    (module
      (func (export "core_add") (param i32 i32) (result i32)
        local.get 0
        local.get 1
        i32.add))
    "#,
        &mut store,
        &registry,
    )
    .await;

    let mut linker = ComponentLinker::new();
    linker.register_export_core("add", core, "core_add");

    let instance = engine
        .instantiate(&program, &mut store, &linker)
        .await
        .expect("instantiate should succeed");

    let result = instance
        .call(
            &mut store,
            "add",
            &[ComponentValue::I32(20), ComponentValue::I32(22)],
        )
        .await
        .expect("core call should succeed");

    assert_eq!(result, vec![ComponentValue::I32(42)]);
}

#[tokio::test]
async fn component_runtime_nested_component_instantiation_succeeds() {
    let bytes = compile_component(
        r#"
(component
  (type
    (component
      (type (func (result u32)))
      (import "x" (func (type 0)))
    )
  )
  (import "b" (component (type 0)))
  (component
    (type
      (component
        (type (func (result u32)))
        (import "x" (func (type 0)))
      )
    )
    (import "a" (component (type 0)))
  )
  (instance (instantiate 1 (with "a" (component 0))))
)
"#,
    );

    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");
    let mut store = telomere::Store::new();
    let linker = ComponentLinker::new();

    let _instance = engine
        .instantiate(&program, &mut store, &linker)
        .await
        .expect("instantiate should succeed");
}

#[tokio::test]
async fn component_runtime_inline_instance_export_instantiation_succeeds() {
    let bytes = compile_component(
        r#"
(component
  (component)
  (instance (instantiate 0))
  (instance (export "b" (instance 0)))
)
"#,
    );

    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");
    let mut store = telomere::Store::new();
    let linker = ComponentLinker::new();

    let _instance = engine
        .instantiate(&program, &mut store, &linker)
        .await
        .expect("instantiate should succeed");
}

#[tokio::test]
async fn component_runtime_sync_wrapper_registration_still_works() {
    let bytes = compile_component(
        r#"
(component
  (type (func (param "lhs" s32) (param "rhs" s32) (result s32)))
  (import "host-add" (func (type 0)))
  (export "add" (func 0))
)
"#,
    );

    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");

    let mut linker = ComponentLinker::new();
    linker.register_export("add", |_store, args| {
        if args.len() != 2 {
            return Err(ComponentError::InvalidArgument(
                "add expects exactly 2 arguments".to_owned(),
            ));
        }
        let lhs = args[0]
            .as_i32()
            .ok_or_else(|| ComponentError::InvalidArgument("arg[0] must be i32".to_owned()))?;
        let rhs = args[1]
            .as_i32()
            .ok_or_else(|| ComponentError::InvalidArgument("arg[1] must be i32".to_owned()))?;
        Ok(vec![ComponentValue::I32(lhs + rhs)])
    });

    let mut store = telomere::Store::new();
    let instance = engine
        .instantiate(&program, &mut store, &linker)
        .await
        .expect("instantiate should succeed");

    let result = instance
        .call(
            &mut store,
            "add",
            &[ComponentValue::I32(1), ComponentValue::I32(2)],
        )
        .await
        .expect("call should succeed");

    assert_eq!(result, vec![ComponentValue::I32(3)]);
}
