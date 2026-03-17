use std::{sync::Arc, thread};

use futures::executor::block_on;
use telomere::{
    aliasing, get_global, instantiate, link_host_function_with_export_name,
    link_host_function_with_function_idx, run_module_function, IoReadBinaryReader, Registry,
    ResultValue, Store, VMResult, WasmParser, WasmValue,
};
use telomere::{
    common::{ExecuteContext, FuncType, HostFunctionDefinition, Instr, NativeModule},
    runtime::instantiate_native_module,
};

fn assert_send_sync<T: Send + Sync>() {}

fn parse_module(wat: &str) -> telomere::Module {
    let source = wat::parse_str(wat).expect("wat must parse");
    let mut reader = IoReadBinaryReader::from(&source[..]);
    let mut parser = WasmParser::new(&mut reader);
    parser.parse_module().expect("module must parse")
}

async fn instantiate_wat(
    wat: &str,
    store: &Store,
    registry: &Registry,
) -> telomere::common::InstanceHandle {
    instantiate(parse_module(wat), store, registry)
        .await
        .unwrap()
}

fn noop_host(ctx: &mut ExecuteContext) -> VMResult<*const Instr> {
    let (prev_local_ref, return_addr) = ctx.stack.function_return(&ctx.local_reference, 0, ctx.gc);
    ctx.local_reference = prev_local_ref;
    VMResult::Success(return_addr)
}

#[test]
fn store_and_instance_handles_are_send_sync() {
    assert_send_sync::<Store>();
    assert_send_sync::<telomere::common::InstanceHandle>();
    assert_send_sync::<Arc<Store>>();
}

#[tokio::test]
async fn instance_handles_survive_aliasing_registry_imports_and_get_global() {
    let store = Store::new();
    let mut registry = Registry::new();

    let producer = instantiate_wat(
        r#"
        (module
          (global (export "g") i32 (i32.const 42))
          (func (export "answer") (result i32)
            global.get 0))
        "#,
        &store,
        &registry,
    )
    .await;
    registry.register("producer", producer);

    let consumer = instantiate_wat(
        r#"
        (module
          (import "producer" "answer" (func $answer (result i32)))
          (import "producer" "g" (global $g i32))
          (func (export "call") (result i32)
            call $answer
            global.get $g
            i32.add))
        "#,
        &store,
        &registry,
    )
    .await;

    let alias = aliasing(
        &registry,
        &[
            (
                "producer".to_owned(),
                "answer".to_owned(),
                "answer_alias".to_owned(),
            ),
            ("producer".to_owned(), "g".to_owned(), "g_alias".to_owned()),
        ],
        &store,
    )
    .unwrap();

    let consumer_result = run_module_function(&consumer, &store, "call", &ResultValue::new(vec![]))
        .await
        .unwrap();
    assert_eq!(consumer_result, ResultValue::new(vec![WasmValue::I32(84)]));

    let alias_result =
        run_module_function(&alias, &store, "answer_alias", &ResultValue::new(vec![]))
            .await
            .unwrap();
    assert_eq!(alias_result, ResultValue::new(vec![WasmValue::I32(42)]));

    let alias_global = get_global(&alias, &store, "g_alias").unwrap();
    assert_eq!(alias_global, WasmValue::I32(42));
}

#[test]
fn store_supports_parallel_calls_via_arc() {
    let store = Arc::new(Store::new());
    let registry = Registry::new();

    let add_instance = block_on(instantiate_wat(
        r#"
        (module
          (func (export "add") (param i32 i32) (result i32)
            local.get 0
            local.get 1
            i32.add))
        "#,
        store.as_ref(),
        &registry,
    ));

    let host = block_on(instantiate_native_module(
        NativeModule {
            functions: vec![HostFunctionDefinition {
                fp: noop_host,
                name: Some("noop".to_owned()),
                signature: FuncType::new(vec![], vec![]),
            }],
        },
        store.as_ref(),
        &Registry::new(),
    ))
    .unwrap();

    link_host_function_with_function_idx(&host, 0, noop_host, store.as_ref());
    link_host_function_with_export_name(&host, "noop", noop_host, store.as_ref());

    let mut tasks = Vec::new();

    {
        let store = Arc::clone(&store);
        tasks.push(thread::spawn(move || {
            for _ in 0..32 {
                link_host_function_with_function_idx(&host, 0, noop_host, store.as_ref());
                link_host_function_with_export_name(&host, "noop", noop_host, store.as_ref());
                thread::yield_now();
            }
        }));
    }

    for _ in 0..8 {
        let store = Arc::clone(&store);
        let instance = add_instance.clone();
        tasks.push(thread::spawn(move || {
            for _ in 0..32 {
                let result = block_on(run_module_function(
                    &instance,
                    store.as_ref(),
                    "add",
                    &ResultValue::new(vec![WasmValue::I32(20), WasmValue::I32(22)]),
                ))
                .unwrap();
                assert_eq!(result, ResultValue::new(vec![WasmValue::I32(42)]));
                thread::yield_now();
            }
        }));
    }

    for task in tasks {
        task.join().expect("task must finish without panic");
    }
}
