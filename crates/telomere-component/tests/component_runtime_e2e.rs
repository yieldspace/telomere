mod common;

use common::instantiate_wat;
use telomere::Registry;
use telomere_component::{ComponentEngine, ComponentError, ComponentLinker, ComponentValue};

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

#[tokio::test]
async fn component_runtime_canon_lower_writes_indirect_results_into_caller_area() {
    let bytes = compile_component(
        r#"
(component
  (import "host" (func $host (result string)))
  (core module $libc
    (memory (export "memory") 1)
    (global $bump (mut i32) (i32.const 8))
    (func (export "realloc")
      (param $old_ptr i32) (param $old_len i32) (param $align i32) (param $new_len i32)
      (result i32)
      (local $ptr i32)
      (local.set $ptr (global.get $bump))
      (global.set $bump (i32.add (local.get $ptr) (local.get $new_len)))
      (local.get $ptr)
    )
  )
  (core instance $libc (instantiate $libc))
  (core func $host-lower
    (canon lower (func $host) (memory $libc "memory") (realloc (func $libc "realloc")))
  )
  (core module $caller
    (import "" "host" (func $host (param i32)))
    (func (export "call-host") (result i32)
      (call $host (i32.const 0))
      (i32.const 0)
    )
  )
  (core instance $caller
    (instantiate $caller
      (with "" (instance
        (export "host" (func $host-lower))
      ))
    )
  )
  (func (export "call-host") (result string)
    (canon lift (core func $caller "call-host") (memory $libc "memory"))
  )
)
"#,
    );

    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");
    let mut linker = ComponentLinker::new();
    linker.register_import("host", |_store, args| {
        assert!(
            args.is_empty(),
            "host should not receive component arguments"
        );
        Ok(vec![ComponentValue::String("hello".to_owned())])
    });

    let mut store = telomere::Store::new();
    let instance = engine
        .instantiate(&program, &mut store, &linker)
        .await
        .expect("instantiate should succeed");

    let result = instance
        .call(&mut store, "call-host", &[])
        .await
        .expect("call should succeed");

    assert_eq!(result, vec![ComponentValue::String("hello".to_owned())]);
}

#[tokio::test]
async fn component_runtime_calls_post_return_after_lift() {
    let bytes = compile_component(
        r#"
(component
  (core module $m
    (memory (export "mem") 1)
    (global $count (mut i32) (i32.const 0))
    (func (export "f") (result i32)
      (i32.store (i32.const 0) (i32.const 8))
      (i32.store (i32.const 4) (i32.const 1))
      (i32.store8 (i32.const 8) (i32.const 97))
      (i32.const 0)
    )
    (func (export "p") (param i32)
      (global.set $count (i32.add (global.get $count) (i32.const 1)))
    )
    (func (export "count") (result i32)
      (global.get $count)
    )
  )
  (core instance $i (instantiate $m))
  (func (export "str") (result string)
    (canon lift (core func $i "f") (memory $i "mem") (post-return (func $i "p")))
  )
  (func (export "count") (result u32)
    (canon lift (core func $i "count"))
  )
)
"#,
    );

    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");
    let mut store = telomere::Store::new();
    let linker = ComponentLinker::new();
    let instance = engine
        .instantiate(&program, &mut store, &linker)
        .await
        .expect("instantiate should succeed");

    let result = instance
        .call(&mut store, "str", &[])
        .await
        .expect("call should succeed");
    assert_eq!(result, vec![ComponentValue::String("a".to_owned())]);

    let count = instance
        .call(&mut store, "count", &[])
        .await
        .expect("count should succeed");
    assert_eq!(count, vec![ComponentValue::U32(1)]);
}

#[tokio::test]
async fn component_runtime_surfaces_post_return_traps() {
    let bytes = compile_component(
        r#"
(component
  (core module $m
    (memory (export "mem") 1)
    (func (export "f") (result i32)
      (i32.store (i32.const 0) (i32.const 8))
      (i32.store (i32.const 4) (i32.const 1))
      (i32.store8 (i32.const 8) (i32.const 97))
      (i32.const 0)
    )
    (func (export "p") (param i32)
      unreachable
    )
  )
  (core instance $i (instantiate $m))
  (func (export "str") (result string)
    (canon lift (core func $i "f") (memory $i "mem") (post-return (func $i "p")))
  )
)
"#,
    );

    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");
    let mut store = telomere::Store::new();
    let linker = ComponentLinker::new();
    let instance = engine
        .instantiate(&program, &mut store, &linker)
        .await
        .expect("instantiate should succeed");

    let error = instance
        .call(&mut store, "str", &[])
        .await
        .expect_err("post-return trap should surface");
    assert!(matches!(error, ComponentError::Trap(message) if message.contains("unreachable")));
}

#[tokio::test]
async fn component_runtime_surfaces_resource_drop_destructor_traps() {
    let bytes = compile_component(
        r#"
(component
  (core module $m
    (func (export "dtor") (param i32)
      unreachable
    )
  )
  (core instance $m (instantiate $m))
  (type $r (resource (rep i32) (dtor (func $m "dtor"))))
  (core func $new (canon resource.new $r))
  (core func $drop (canon resource.drop $r))
  (core module $caller
    (import "" "new" (func $new (param i32) (result i32)))
    (import "" "drop" (func $drop (param i32)))
    (func (export "run")
      (call $drop (call $new (i32.const 7)))
    )
  )
  (core instance $caller
    (instantiate $caller
      (with "" (instance
        (export "new" (func $new))
        (export "drop" (func $drop))
      ))
    )
  )
  (func (export "run")
    (canon lift (core func $caller "run"))
  )
)
"#,
    );

    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");
    let mut store = telomere::Store::new();
    let linker = ComponentLinker::new();
    let instance = engine
        .instantiate(&program, &mut store, &linker)
        .await
        .expect("instantiate should succeed");

    let error = instance
        .call(&mut store, "run", &[])
        .await
        .expect_err("resource drop trap should surface");
    assert!(matches!(error, ComponentError::Trap(message) if message.contains("unreachable")));
}
