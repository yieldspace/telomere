//! Regression tests for #324: inline core instances `(instance (export
//! ...) ...)` must support global exports, like they already supported
//! func/memory/table exports. Before the fix, materialising such an
//! instance failed at runtime with "runtime inline core instances only
//! support func/memory/table exports" (upstream `linking/unit.wast`
//! lines 2010 and 2043, with the knock-on invoke failures at lines
//! 2011-2012). The materialised instance must alias the provider's
//! global itself, not copy its value, so live updates to a mutable
//! global flow through.

use telomere_component::{ComponentEngine, ComponentLinker, ComponentValue};

fn compile_component(text: &str) -> Vec<u8> {
    wat::parse_str(text).expect("component wat must be valid")
}

fn compile_result(text: &str) -> Result<(), String> {
    let bytes = compile_component(text);
    let engine = ComponentEngine::new();
    engine
        .compile(&bytes)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

async fn instantiate_root(wat: &str) -> (telomere::Store, telomere_component::ComponentInstance) {
    let bytes = compile_component(wat);
    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");
    let store = telomere::Store::new();
    let linker = ComponentLinker::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");
    (store, instance)
}

async fn call_u32(
    store: &telomere::Store,
    instance: &telomere_component::ComponentInstance,
    name: &str,
) -> u32 {
    let result = instance
        .call(store, name, &[])
        .await
        .expect("call should succeed");
    match result.as_slice() {
        [ComponentValue::U32(value)] => *value,
        other => panic!("expected a single u32 result from {name}, got {other:?}"),
    }
}

async fn call_unit(
    store: &telomere::Store,
    instance: &telomere_component::ComponentInstance,
    name: &str,
) {
    let result = instance
        .call(store, name, &[])
        .await
        .expect("call should succeed");
    assert!(
        result.is_empty(),
        "expected no results from {name}, got {result:?}"
    );
}

/// Upstream `linking/unit.wast` lines 1993-2012: an immutable global is
/// aliased out of a provider instance and fed, together with a direct
/// reference to the provider's mutable i64 global export, through an
/// inline core instance into a second module. The mutable global is
/// shared, so `bump64` on the provider is visible to the reader.
#[tokio::test]
async fn inline_core_instance_provides_aliased_and_direct_globals() {
    const WAT: &str = r#"
      (component
        (core module $P
          (global (export "gi") i32 (i32.const 30))
          (global $g64 (export "g64") (mut i64) (i64.const 7))
          (func (export "bump64")
            (global.set $g64 (i64.add (global.get $g64) (i64.const 1)))))
        (core instance $p (instantiate $P))
        (alias core export $p "gi" (core global $gi))
        (core module $User
          (import "env" "base" (global $base i32))
          (import "env" "big" (global $big (mut i64)))
          (func (export "read") (result i32)
            (i32.add (global.get $base) (i32.wrap_i64 (global.get $big)))))
        (core instance $u (instantiate $User
          (with "env" (instance
            (export "base" (global $gi))
            (export "big" (global $p "g64"))))))
        (func (export "bump64") (canon lift (core func $p "bump64")))
        (func (export "read") (result u32) (canon lift (core func $u "read"))))
    "#;
    let (store, instance) = instantiate_root(WAT).await;
    assert_eq!(call_u32(&store, &instance, "read").await, 37);
    call_unit(&store, &instance, "bump64").await;
    assert_eq!(call_u32(&store, &instance, "read").await, 38);
}

/// Upstream `linking/unit.wast` lines 2016-2043: one provider instance's
/// memory, global, and table exports are renamed through a synthesized
/// (inline) core instance to satisfy a client's differently-named
/// imports of every non-func kind.
#[tokio::test]
async fn inline_core_instance_renames_global_with_memory_and_table() {
    const WAT: &str = r#"
      (component
        (core module $P
          (memory (export "1") 1)
          (data (i32.const 0) "\05")
          (global (export "2") i32 (i32.const 300))
          (func $forty (result i32) (i32.const 40))
          (table (export "3") 1 funcref)
          (elem (i32.const 0) func $forty))
        (core instance $p (instantiate $P))
        (core module $Client
          (type $t (func (result i32)))
          (import "env" "mem" (memory 1))
          (import "env" "glob" (global $g i32))
          (import "env" "tab" (table 1 funcref))
          (func (export "sum") (result i32)
            (i32.add
              (i32.add (i32.load8_u (i32.const 0)) (global.get $g))
              (call_indirect (type $t) (i32.const 0)))))
        (core instance $c (instantiate $Client
          (with "env" (instance
            (export "mem" (memory $p "1"))
            (export "glob" (global $p "2"))
            (export "tab" (table $p "3"))))))
        (func (export "sum") (result u32) (canon lift (core func $c "sum"))))
    "#;
    let (store, instance) = instantiate_root(WAT).await;
    assert_eq!(call_u32(&store, &instance, "sum").await, 345);
}

/// The global an inline instance provides must match the importing
/// module's declared global type: an i64 global does not satisfy an
/// i32 global import.
#[tokio::test]
async fn inline_core_instance_global_value_type_mismatch_is_rejected() {
    const WAT: &str = r#"
      (component
        (core module $P
          (global (export "g") i64 (i64.const 7)))
        (core instance $p (instantiate $P))
        (core module $User
          (import "env" "g" (global i32))
          (func (export "read") (result i32) (global.get 0)))
        (core instance $u (instantiate $User
          (with "env" (instance (export "g" (global $p "g"))))))
        (func (export "read") (result u32) (canon lift (core func $u "read"))))
    "#;
    let error = compile_result(WAT).expect_err("value type mismatch must be rejected");
    assert!(
        error.contains("mismatch"),
        "expected a type mismatch error, got: {error}"
    );
}

/// Mutability is part of the global type: an immutable global does not
/// satisfy a mutable global import.
#[tokio::test]
async fn inline_core_instance_global_mutability_mismatch_is_rejected() {
    const WAT: &str = r#"
      (component
        (core module $P
          (global (export "g") i64 (i64.const 7)))
        (core instance $p (instantiate $P))
        (core module $User
          (import "env" "g" (global (mut i64)))
          (func (export "read") (result i64) (global.get 0)))
        (core instance $u (instantiate $User
          (with "env" (instance (export "g" (global $p "g"))))))
        (func (export "read") (result u64) (canon lift (core func $u "read"))))
    "#;
    let error = compile_result(WAT).expect_err("mutability mismatch must be rejected");
    assert!(
        error.contains("mismatch"),
        "expected a type mismatch error, got: {error}"
    );
}

/// Sorts must line up too: a global export does not satisfy a memory
/// import of the same name.
#[tokio::test]
async fn inline_core_instance_global_does_not_satisfy_memory_import() {
    const WAT: &str = r#"
      (component
        (core module $P
          (global (export "x") i32 (i32.const 7)))
        (core instance $p (instantiate $P))
        (core module $User
          (import "env" "x" (memory 1))
          (func (export "read") (result i32) (i32.load8_u (i32.const 0))))
        (core instance $u (instantiate $User
          (with "env" (instance (export "x" (global $p "x"))))))
        (func (export "read") (result u32) (canon lift (core func $u "read"))))
    "#;
    let error = compile_result(WAT).expect_err("sort mismatch must be rejected");
    assert!(
        error.contains("mismatch"),
        "expected a type mismatch error, got: {error}"
    );
}
