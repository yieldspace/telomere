mod common;

use common::instantiate_wat;
use telomere::Registry;
use telomere_component::{
    ComponentEngine, ComponentError, ComponentLinker, ComponentLinkerInstance, ComponentValue,
};

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

    let store = telomere::Store::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");

    let result = instance
        .call(
            &store,
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
    let store = telomere::Store::new();
    let registry = Registry::new();
    let core = instantiate_wat(
        r#"
    (module
      (func (export "core_add") (param i32 i32) (result i32)
        local.get 0
        local.get 1
        i32.add))
    "#,
        &store,
        &registry,
    )
    .await;

    let mut linker = ComponentLinker::new();
    linker.register_export_core("add", core, "core_add");

    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");

    let result = instance
        .call(
            &store,
            "add",
            &[ComponentValue::I32(20), ComponentValue::I32(22)],
        )
        .await
        .expect("core call should succeed");

    assert_eq!(result, vec![ComponentValue::I32(42)]);
}

#[tokio::test]
async fn component_runtime_can_lower_registered_core_imports_back_into_core() {
    let bytes = compile_component(
        r#"
(component
  (type (func (param "lhs" s32) (param "rhs" s32) (result s32)))
  (import "core-add" (func $core-add (type 0)))
  (core func $core-add-lower
    (canon lower (func $core-add))
  )
  (core module $caller
    (import "" "core-add" (func $core-add (param i32 i32) (result i32)))
    (func (export "call-add") (param i32 i32) (result i32)
      local.get 0
      local.get 1
      call $core-add)
  )
  (core instance $caller
    (instantiate $caller
      (with "" (instance
        (export "core-add" (func $core-add-lower))
      ))
    )
  )
  (func (export "call-add") (param "lhs" s32) (param "rhs" s32) (result s32)
    (canon lift (core func $caller "call-add"))
  )
)
"#,
    );

    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");
    let store = telomere::Store::new();
    let registry = Registry::new();
    let core = instantiate_wat(
        r#"
    (module
      (func (export "core_add") (param i32 i32) (result i32)
        local.get 0
        local.get 1
        i32.add))
    "#,
        &store,
        &registry,
    )
    .await;

    let mut linker = ComponentLinker::new();
    linker.register_import_core("core-add", core, "core_add");

    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");

    let result = instance
        .call(
            &store,
            "call-add",
            &[ComponentValue::I32(20), ComponentValue::I32(22)],
        )
        .await
        .expect("call should succeed");

    assert_eq!(result, vec![ComponentValue::S32(42)]);
}

#[tokio::test]
async fn component_runtime_lowers_host_owned_resource_without_params() {
    let bytes = compile_component(
        r#"
(component
  (import "output-stream" (type $resource (sub resource)))
  (type $get-stdout (func (result (own $resource))))
  (import "[static]output-stream.get-stdout" (func $get-stdout (type $get-stdout)))
  (core func $get-stdout-lower
    (canon lower (func $get-stdout))
  )
  (core module $caller
    (import "" "get-stdout" (func $get-stdout (result i32)))
    (func (export "call") (result i32)
      (call $get-stdout)
    )
  )
  (core instance $caller
    (instantiate $caller
      (with "" (instance
        (export "get-stdout" (func $get-stdout-lower))
      ))
    )
  )
  (func (export "call") (result (own $resource))
    (canon lift (core func $caller "call"))
  )
)
"#,
    );

    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");
    let mut linker = ComponentLinker::new();
    linker.register_import("[static]output-stream.get-stdout", |_store, args| {
        assert!(args.is_empty());
        Ok(vec![ComponentValue::Own(42)])
    });

    let store = telomere::Store::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");

    let result = instance
        .call(&store, "call", &[])
        .await
        .expect("call should succeed");

    assert_eq!(result, vec![ComponentValue::Own(42)]);
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
    (type (func (result u32)))
    (import "x" (func (type 0)))
  )
  (component
    (type
      (component
        (type (func (result u32)))
        (import "x" (func (type 0)))
      )
    )
    (import "a" (component (type 0)))
  )
  (instance (instantiate 2 (with "a" (component 1))))
)
"#,
    );

    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");
    let store = telomere::Store::new();
    let linker = ComponentLinker::new();

    let _instance = engine
        .instantiate(&program, &store, &linker)
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
    let store = telomere::Store::new();
    let linker = ComponentLinker::new();

    let _instance = engine
        .instantiate(&program, &store, &linker)
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

    let store = telomere::Store::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");

    let result = instance
        .call(
            &store,
            "add",
            &[ComponentValue::I32(1), ComponentValue::I32(2)],
        )
        .await
        .expect("call should succeed");

    assert_eq!(result, vec![ComponentValue::I32(3)]);
}

fn reexport_component(version: &str) -> Vec<u8> {
    compile_component(&format!(
        r#"
(component
  (type (func (param "lhs" s32) (param "rhs" s32) (result s32)))
  (import "test:pkg/add@{version}" (func (type 0)))
  (export "test:pkg/add@{version}" (func 0))
)
"#
    ))
}

fn add_impl(
    _store: &telomere::Store,
    args: &[ComponentValue],
) -> Result<Vec<ComponentValue>, ComponentError> {
    let lhs = args[0]
        .as_i32()
        .ok_or_else(|| ComponentError::InvalidArgument("arg[0] must be i32".to_owned()))?;
    let rhs = args[1]
        .as_i32()
        .ok_or_else(|| ComponentError::InvalidArgument("arg[1] must be i32".to_owned()))?;
    Ok(vec![ComponentValue::I32(lhs + rhs)])
}

#[tokio::test]
async fn component_runtime_host_export_resolves_through_compatible_version_name() {
    // The component re-exports its import as `test:pkg/add@0.2.0` and the
    // embedder supplies that export through `register_export` under the
    // same name. Calling through the semver-compatible name
    // `test:pkg/add@0.2.6` must reach the registered host-export binding
    // (#271); before the fix the lookup passed the requested name to the
    // linker, missed the registration, and fell through to import
    // resolution, which reported an unresolved import.
    let engine = ComponentEngine::new();
    let store = telomere::Store::new();

    let bytes = reexport_component("0.2.0");
    let program = engine.compile(&bytes).expect("compile should succeed");
    let mut linker = ComponentLinker::new();
    linker.register_export("test:pkg/add@0.2.0", add_impl);
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");
    let result = instance
        .call(
            &store,
            "test:pkg/add@0.2.6",
            &[ComponentValue::I32(20), ComponentValue::I32(22)],
        )
        .await
        .expect("compatible-name call should reach the host-export binding");
    assert_eq!(result, vec![ComponentValue::I32(42)]);

    // The reverse direction resolves too: export and registration at
    // 0.2.6, call at 0.2.0.
    let bytes = reexport_component("0.2.6");
    let program = engine.compile(&bytes).expect("compile should succeed");
    let mut linker = ComponentLinker::new();
    linker.register_export("test:pkg/add@0.2.6", add_impl);
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");
    let result = instance
        .call(
            &store,
            "test:pkg/add@0.2.0",
            &[ComponentValue::I32(20), ComponentValue::I32(22)],
        )
        .await
        .expect("compatible-name call should reach the host-export binding");
    assert_eq!(result, vec![ComponentValue::I32(42)]);
}

#[tokio::test]
async fn component_runtime_host_export_wins_over_import_binding_for_compatible_name() {
    // When a separate import binding also exists, a compatible-name call
    // to a re-exported import must still invoke the host-export binding
    // registered for the export, not the import binding (#271).
    let bytes = reexport_component("0.2.0");
    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");

    let mut linker = ComponentLinker::new();
    linker.register_export("test:pkg/add@0.2.0", add_impl);
    linker.register_import("test:pkg/add@0.2.0", |_store, _args| {
        Ok(vec![ComponentValue::I32(-1)])
    });

    let store = telomere::Store::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");
    let result = instance
        .call(
            &store,
            "test:pkg/add@0.2.6",
            &[ComponentValue::I32(20), ComponentValue::I32(22)],
        )
        .await
        .expect("compatible-name call should succeed");
    assert_eq!(
        result,
        vec![ComponentValue::I32(42)],
        "the host-export binding must win over the import binding"
    );
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

    let store = telomere::Store::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");

    let result = instance
        .call(&store, "call-host", &[])
        .await
        .expect("call should succeed");

    assert_eq!(result, vec![ComponentValue::String("hello".to_owned())]);
}

#[tokio::test]
async fn component_runtime_canon_lower_reads_packed_u8_lists_from_guest_memory() {
    let bytes = compile_component(
        r#"
(component
  (import "host" (func $host (param "bytes" (list u8))))
  (core module $guest
    (memory (export "memory") 1)
    (data (i32.const 8) "Hello, world!\n")
  )
  (core instance $guest (instantiate $guest))
  (core func $host-lower
    (canon lower (func $host) (memory $guest "memory"))
  )
  (core module $caller
    (import "" "host" (func $host (param i32 i32)))
    (func (export "call-host")
      (call $host (i32.const 8) (i32.const 14))
    )
  )
  (core instance $caller
    (instantiate $caller
      (with "" (instance
        (export "host" (func $host-lower))
      ))
    )
  )
  (func (export "call-host")
    (canon lift (core func $caller "call-host") (memory $guest "memory"))
  )
)
"#,
    );

    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");
    let mut linker = ComponentLinker::new();
    linker.register_import("host", |_store, args| {
        assert_eq!(
            args,
            &[ComponentValue::List(
                b"Hello, world!\n"
                    .iter()
                    .copied()
                    .map(ComponentValue::U8)
                    .collect()
            )]
        );
        Ok(Vec::new())
    });

    let store = telomere::Store::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");

    instance
        .call(&store, "call-host", &[])
        .await
        .expect("call should succeed");
}

#[tokio::test]
async fn component_runtime_fixup_populates_shim_table_for_host_lowerings() {
    let bytes = compile_component(
        r#"
(component
  (import "host" (func $host (param "value" u32) (result u32)))
  (core module $shim
    (type $thunk (func (param i32) (result i32)))
    (table (export "imports") 1 1 funcref)
    (func (export "wrapper") (param i32) (result i32)
      (local.get 0)
      (i32.const 0)
      (call_indirect (type $thunk))
    )
  )
  (core instance $shim (instantiate $shim))
  (core func $host-lower
    (canon lower (func $host))
  )
  (core instance $fixup-args
    (export "imports" (table $shim "imports"))
    (export "host" (func $host-lower))
  )
  (core module $fixup
    (type $thunk (func (param i32) (result i32)))
    (import "" "host" (func $host (type $thunk)))
    (import "" "imports" (table 1 1 funcref))
    (elem (i32.const 0) func $host)
  )
  (core instance $fixup
    (instantiate $fixup
      (with "" (instance $fixup-args))
    )
  )
  (func (export "call") (param "value" u32) (result u32)
    (canon lift (core func $shim "wrapper"))
  )
)
"#,
    );

    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");
    let mut linker = ComponentLinker::new();
    linker.register_import("host", |_store, args| {
        assert_eq!(args, &[ComponentValue::U32(7)]);
        Ok(vec![ComponentValue::U32(42)])
    });

    let store = telomere::Store::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");

    let result = instance
        .call(&store, "call", &[ComponentValue::U32(7)])
        .await
        .expect("call should succeed");

    assert_eq!(result, vec![ComponentValue::U32(42)]);
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
    let store = telomere::Store::new();
    let linker = ComponentLinker::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");

    let result = instance
        .call(&store, "str", &[])
        .await
        .expect("call should succeed");
    assert_eq!(result, vec![ComponentValue::String("a".to_owned())]);

    let count = instance
        .call(&store, "count", &[])
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
    let store = telomere::Store::new();
    let linker = ComponentLinker::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");

    let error = instance
        .call(&store, "str", &[])
        .await
        .expect_err("post-return trap should surface");
    assert!(
        matches!(error, ComponentError::GuestTrap { kind: telomere::TrapKind::Unreachable, ref message } if message.contains("unreachable"))
    );
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
    let store = telomere::Store::new();
    let linker = ComponentLinker::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");

    let error = instance
        .call(&store, "run", &[])
        .await
        .expect_err("resource drop trap should surface");
    assert!(
        matches!(error, ComponentError::GuestTrap { kind: telomere::TrapKind::Unreachable, ref message } if message.contains("unreachable"))
    );
}

fn versioned_instance_component(version: &str) -> Vec<u8> {
    compile_component(&format!(
        r#"
(component
  (type $get-func (func (result s32)))

  (core module $get-core
    (func (export "get") (result i32)
      i32.const 42)
  )
  (core instance $get-core-inst (instantiate $get-core))
  (func $get (type $get-func)
    (canon lift (core func $get-core-inst "get")))

  (instance $iface-export
    (export "get" (func $get))
  )
  (export "test:pkg/iface@{version}" (instance $iface-export))
)
"#
    ))
}

#[tokio::test]
async fn component_export_lookup_accepts_semver_compatible_version() {
    // The component exports the interface at 0.2.0; the caller asks for
    // 0.2.6. Both are on the 0.2.x track, so the lookup resolves (#117).
    let bytes = versioned_instance_component("0.2.0");
    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");
    let store = telomere::Store::new();
    let linker = ComponentLinker::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");

    let result = instance
        .get_instance("test:pkg/iface@0.2.6")
        .call(&store, "get", &[])
        .await
        .expect("semver-compatible export lookup should succeed");
    assert_eq!(result, vec![ComponentValue::S32(42)]);
}

#[tokio::test]
async fn component_export_lookup_rejects_incompatible_version_with_found_and_expected() {
    // 0.3.0 is a different track from the requested 0.2.6, so the lookup is
    // still rejected, and the error names both versions (#117).
    let bytes = versioned_instance_component("0.3.0");
    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");
    let store = telomere::Store::new();
    let linker = ComponentLinker::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");

    let error = instance
        .get_instance("test:pkg/iface@0.2.6")
        .call(&store, "get", &[])
        .await
        .expect_err("incompatible export version must be rejected");
    let message = error.to_string();
    assert!(
        message.contains("test:pkg/iface@0.2.6"),
        "error should name the expected version: {message}"
    );
    assert!(
        message.contains("test:pkg/iface@0.3.0"),
        "error should name the found version: {message}"
    );
}

fn versioned_import_component(version: &str) -> Vec<u8> {
    compile_component(&format!(
        r#"
(component
  (type $get-func (func (result s32)))
  (type $iface (instance (export "get" (func (type $get-func)))))

  (import "test:pkg/iface@{version}" (instance $i (type $iface)))
  (alias export $i "get" (func $get))
  (export "run" (func $get))
)
"#
    ))
}

fn get_instance_returning(value: i32) -> ComponentLinkerInstance {
    let mut instance = ComponentLinkerInstance::new();
    instance.register_func("get", move |_store, _args| {
        Ok(vec![ComponentValue::S32(value)])
    });
    instance
}

async fn call_versioned_import(
    guest_version: &str,
    linker: ComponentLinker,
) -> Result<Vec<ComponentValue>, ComponentError> {
    let bytes = versioned_import_component(guest_version);
    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");
    let store = telomere::Store::new();
    let instance = engine.instantiate(&program, &store, &linker).await?;
    instance.call(&store, "run", &[]).await
}

#[tokio::test]
async fn component_import_lookup_accepts_semver_compatible_version() {
    // The guest imports the interface at 0.2.6; the host registered it at
    // 0.2.0. Both are on the 0.2.x track, so the import resolves (#258).
    let mut linker = ComponentLinker::new();
    linker.register_import_instance("test:pkg/iface@0.2.0", get_instance_returning(42));
    let result = call_versioned_import("0.2.6", linker)
        .await
        .expect("semver-compatible import lookup should succeed");
    assert_eq!(result, vec![ComponentValue::S32(42)]);

    // And the reverse direction resolves too: the guest imports 0.2.0 and
    // the host registered 0.2.6.
    let mut linker = ComponentLinker::new();
    linker.register_import_instance("test:pkg/iface@0.2.6", get_instance_returning(42));
    let result = call_versioned_import("0.2.0", linker)
        .await
        .expect("semver-compatible import lookup should succeed");
    assert_eq!(result, vec![ComponentValue::S32(42)]);
}

#[tokio::test]
async fn component_import_lookup_prefers_exact_registration() {
    let mut linker = ComponentLinker::new();
    linker.register_import_instance("test:pkg/iface@0.2.0", get_instance_returning(20));
    linker.register_import_instance("test:pkg/iface@0.2.6", get_instance_returning(26));
    // An exact registration wins over the maximal version on the track.
    let result = call_versioned_import("0.2.0", linker.clone())
        .await
        .expect("exact import lookup should succeed");
    assert_eq!(result, vec![ComponentValue::S32(20)]);
    // Without an exact registration, the maximal version on the track wins.
    let result = call_versioned_import("0.2.3", linker)
        .await
        .expect("track import lookup should succeed");
    assert_eq!(result, vec![ComponentValue::S32(26)]);
}

#[tokio::test]
async fn component_import_lookup_rejects_incompatible_version_with_found_and_expected() {
    // 0.3.0 is a different track from the imported 0.2.6, so the import is
    // still rejected, and the error names both versions (#258).
    let mut linker = ComponentLinker::new();
    linker.register_import_instance("test:pkg/iface@0.3.0", get_instance_returning(42));
    let error = call_versioned_import("0.2.6", linker)
        .await
        .expect_err("incompatible import version must be rejected");
    let message = error.to_string();
    assert!(
        message.contains("test:pkg/iface@0.2.6"),
        "error should name the expected version: {message}"
    );
    assert!(
        message.contains("test:pkg/iface@0.3.0"),
        "error should name the found version: {message}"
    );
}

#[tokio::test]
async fn component_runtime_drops_host_provided_resource() {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    use telomere_component::ComponentLinkerInstance;

    let bytes = compile_component(
        r#"
(component
  (type $iface
    (instance
      (export "thing" (type (sub resource)))
      (export "make" (func (result (own 0))))
    )
  )
  (import "test:host/resources" (instance $i (type $iface)))
  (alias export $i "thing" (type $thing))
  (alias export $i "make" (func $make))

  (core func $make-lower (canon lower (func $make)))
  (core func $drop (canon resource.drop $thing))
  (core module $caller
    (import "" "make" (func $make (result i32)))
    (import "" "drop" (func $drop (param i32)))
    (func (export "drop-it") (result i32)
      (call $drop (call $make))
      (i32.const 0)
    )
    (func (export "drop-twice") (result i32)
      (local $h i32)
      (local.set $h (call $make))
      (call $drop (local.get $h))
      (call $drop (local.get $h))
      (i32.const 0)
    )
    (func (export "drop-bogus") (result i32)
      (call $drop (i32.const 9999))
      (i32.const 0)
    )
  )
  (core instance $caller
    (instantiate $caller
      (with "" (instance
        (export "make" (func $make-lower))
        (export "drop" (func $drop))
      ))
    )
  )
  (func (export "drop-it") (result s32)
    (canon lift (core func $caller "drop-it"))
  )
  (func (export "drop-twice") (result s32)
    (canon lift (core func $caller "drop-twice"))
  )
  (func (export "drop-bogus") (result s32)
    (canon lift (core func $caller "drop-bogus"))
  )
)
"#,
    );

    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");

    let next_handle = Rc::new(Cell::new(100_u32));
    let dropped = Rc::new(RefCell::new(Vec::new()));

    let mut host_instance = ComponentLinkerInstance::new();
    let counter = Rc::clone(&next_handle);
    host_instance.register_func("make", move |_store, _args| {
        let handle = counter.get();
        counter.set(handle + 1);
        Ok(vec![ComponentValue::Own(handle)])
    });

    let mut linker = ComponentLinker::new();
    linker.register_import_instance("test:host/resources", host_instance);
    let dropped_by_host = Rc::clone(&dropped);
    linker.register_import_resource_drop("test:host/resources", "thing", move |_store, handle| {
        dropped_by_host.borrow_mut().push(handle);
        Ok(())
    });

    let store = telomere::Store::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");

    let result = instance
        .call(&store, "drop-it", &[])
        .await
        .expect("dropping a host-provided resource should succeed");
    assert_eq!(result, vec![ComponentValue::S32(0)]);
    assert_eq!(
        *dropped.borrow(),
        vec![100],
        "the host destructor should run exactly once, with the issued handle"
    );

    let error = instance
        .call(&store, "drop-twice", &[])
        .await
        .expect_err("dropping the same handle twice should trap");
    assert!(
        matches!(error, ComponentError::GuestTrap { kind: telomere::TrapKind::Unreachable, ref message } if message.contains("unreachable")),
        "double drop should trap, got {error:?}"
    );
    assert_eq!(
        *dropped.borrow(),
        vec![100, 101],
        "the first drop releases the handle; the second must not reach the host again"
    );

    let error = instance
        .call(&store, "drop-bogus", &[])
        .await
        .expect_err("dropping a handle that was never issued should trap");
    assert!(
        matches!(error, ComponentError::GuestTrap { kind: telomere::TrapKind::Unreachable, ref message } if message.contains("unreachable")),
        "dropping an unknown handle should trap, got {error:?}"
    );
    assert_eq!(*dropped.borrow(), vec![100, 101]);
}

/// Builds a host future that returns `Poll::Pending` on its first poll and
/// would only complete on a second one, counting how often it was polled.
fn yield_once_future(
    polls: std::rc::Rc<std::cell::Cell<u32>>,
) -> telomere_component::ComponentFuture<'static, Result<Vec<ComponentValue>, ComponentError>> {
    Box::pin(std::future::poll_fn(move |_cx| {
        polls.set(polls.get() + 1);
        if polls.get() == 1 {
            std::task::Poll::Pending
        } else {
            std::task::Poll::Ready(Ok(vec![ComponentValue::I32(42)]))
        }
    }))
}

#[tokio::test]
async fn component_runtime_async_import_that_yields_fails_with_first_poll_error() {
    // Pins the first-poll contract (#147): registration and instantiation
    // succeed, and the failure surfaces as a returned error at call time --
    // never as a panic, and the future is not polled a second time.
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

    let polls = std::rc::Rc::new(std::cell::Cell::new(0_u32));
    let polls_in_fn = std::rc::Rc::clone(&polls);
    let mut linker = ComponentLinker::new();
    linker.register_import_async("host-add", move |_store, _args| {
        yield_once_future(std::rc::Rc::clone(&polls_in_fn))
    });

    let store = telomere::Store::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");

    let error = instance
        .call(
            &store,
            "add",
            &[ComponentValue::I32(20), ComponentValue::I32(22)],
        )
        .await
        .expect_err("a host future that yields on its first poll must fail the call");
    match &error {
        ComponentError::Runtime(message) => {
            assert!(
                message.contains("yielded in sync execution"),
                "error should name the sync-execution failure, got: {message}"
            );
            assert!(
                message.contains("first poll"),
                "error should state the first-poll contract, got: {message}"
            );
        }
        other => panic!("expected a runtime error, got {other:?}"),
    }
    assert_eq!(
        polls.get(),
        1,
        "the future must be polled exactly once and never driven further"
    );
}

#[tokio::test]
async fn component_runtime_async_import_awaiting_ready_future_succeeds() {
    // The other half of the first-poll contract: awaiting futures that are
    // already ready completes within the single poll and is supported.
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
        let lhs = args[0].as_i32().unwrap_or(0);
        let rhs = args[1].as_i32().unwrap_or(0);
        Box::pin(async move {
            let lhs = std::future::ready(lhs).await;
            let rhs = std::future::ready(rhs).await;
            Ok(vec![ComponentValue::I32(lhs + rhs)])
        })
    });

    let store = telomere::Store::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");

    let result = instance
        .call(
            &store,
            "add",
            &[ComponentValue::I32(20), ComponentValue::I32(22)],
        )
        .await
        .expect("awaiting ready futures completes on the first poll");
    assert_eq!(result, vec![ComponentValue::I32(42)]);
}

#[tokio::test]
async fn component_runtime_async_import_that_yields_traps_on_canon_lower_path() {
    // On the canon-lower path the host function is reached from inside core
    // execution, whose boundary cannot carry a `ComponentError`; the
    // documented consequence of yielding there is a guest `unreachable`
    // trap (see the first-poll contract, #147).
    let bytes = compile_component(
        r#"
(component
  (type (func (param "lhs" s32) (param "rhs" s32) (result s32)))
  (import "host-add" (func $host-add (type 0)))
  (core func $host-add-lower
    (canon lower (func $host-add))
  )
  (core module $caller
    (import "" "host-add" (func $add (param i32 i32) (result i32)))
    (func (export "call-add") (param i32 i32) (result i32)
      local.get 0
      local.get 1
      call $add)
  )
  (core instance $caller
    (instantiate $caller
      (with "" (instance
        (export "host-add" (func $host-add-lower))
      ))
    )
  )
  (func (export "call-add") (param "lhs" s32) (param "rhs" s32) (result s32)
    (canon lift (core func $caller "call-add"))
  )
)
"#,
    );

    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");

    let polls = std::rc::Rc::new(std::cell::Cell::new(0_u32));
    let polls_in_fn = std::rc::Rc::clone(&polls);
    let mut linker = ComponentLinker::new();
    linker.register_import_async("host-add", move |_store, _args| {
        yield_once_future(std::rc::Rc::clone(&polls_in_fn))
    });

    let store = telomere::Store::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");

    let error = instance
        .call(
            &store,
            "call-add",
            &[ComponentValue::I32(20), ComponentValue::I32(22)],
        )
        .await
        .expect_err("yielding on the canon-lower path must trap the guest");
    assert!(
        matches!(
            error,
            ComponentError::GuestTrap {
                kind: telomere::TrapKind::Unreachable,
                ..
            }
        ),
        "expected an unreachable guest trap, got {error:?}"
    );
    assert_eq!(polls.get(), 1, "the future must be polled exactly once");
}

#[tokio::test]
async fn component_runtime_async_resource_drop_that_yields_traps() {
    // Resource destructors registered with
    // `register_import_resource_drop_async` share the first-poll contract:
    // a destructor future that yields fails the drop, which surfaces as a
    // guest trap because drops execute inside core execution.
    use std::cell::Cell;
    use std::rc::Rc;

    let bytes = compile_component(
        r#"
(component
  (type $iface
    (instance
      (export "thing" (type (sub resource)))
      (export "make" (func (result (own 0))))
    )
  )
  (import "test:host/resources" (instance $i (type $iface)))
  (alias export $i "thing" (type $thing))
  (alias export $i "make" (func $make))

  (core func $make-lower (canon lower (func $make)))
  (core func $drop (canon resource.drop $thing))
  (core module $caller
    (import "" "make" (func $make (result i32)))
    (import "" "drop" (func $drop (param i32)))
    (func (export "drop-it") (result i32)
      (call $drop (call $make))
      (i32.const 0)
    )
  )
  (core instance $caller
    (instantiate $caller
      (with "" (instance
        (export "make" (func $make-lower))
        (export "drop" (func $drop))
      ))
    )
  )
  (func (export "drop-it") (result s32)
    (canon lift (core func $caller "drop-it"))
  )
)
"#,
    );

    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");

    let next_handle = Rc::new(Cell::new(100_u32));
    let mut host_instance = ComponentLinkerInstance::new();
    let counter = Rc::clone(&next_handle);
    host_instance.register_func("make", move |_store, _args| {
        let handle = counter.get();
        counter.set(handle + 1);
        Ok(vec![ComponentValue::Own(handle)])
    });

    let mut linker = ComponentLinker::new();
    linker.register_import_instance("test:host/resources", host_instance);
    linker.register_import_resource_drop_async(
        "test:host/resources",
        "thing",
        |_store, _handle| Box::pin(std::future::pending()),
    );

    let store = telomere::Store::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");

    let error = instance
        .call(&store, "drop-it", &[])
        .await
        .expect_err("a destructor that yields on its first poll must fail the drop");
    assert!(
        matches!(
            error,
            ComponentError::GuestTrap {
                kind: telomere::TrapKind::Unreachable,
                ..
            }
        ),
        "expected an unreachable guest trap, got {error:?}"
    );
}
