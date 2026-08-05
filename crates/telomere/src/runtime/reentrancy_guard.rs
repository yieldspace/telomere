use super::{
    instantiate::{
        self, link_async_host_function_with_export_name_impl,
        link_async_host_function_with_function_idx_impl, link_host_function_with_export_name_impl,
        link_host_function_with_function_idx_impl,
    },
    scheduler::TokioDriver,
    vm,
};
use crate::{
    common::{store::StoreExecutionError, AsyncHostFuture, ExecuteContext, InstanceHandle, Instr},
    IoReadBinaryReader, Module, Registry, ResultValue, Store, VMResult, WasmParser,
};

fn parse_wat_for_test(wat_src: &str) -> Module {
    let bytes = wat::parse_str(wat_src).expect("test module must parse as WAT");
    let mut reader = IoReadBinaryReader::from(bytes.as_slice());
    WasmParser::new(&mut reader)
        .parse_module()
        .expect("test module must parse")
}

async fn store_and_instance_for_test() -> (Store, InstanceHandle) {
    let store = Store::new();
    let registry = Registry::new();
    let module = parse_wat_for_test(
        r#"
        (module
          (global (export "g") i32 (i32.const 7))
          (func (export "run")))
        "#,
    );
    let instance = match instantiate::instantiate(module, &store, &registry).await {
        VMResult::Success(instance) => instance,
        result => panic!("test module must instantiate: {result:?}"),
    };
    (store, instance)
}

fn unreachable_host(_ctx: &mut ExecuteContext<'_>) -> VMResult<*const Instr> {
    VMResult::Unreachable
}

fn async_unreachable_host(_ctx: &mut ExecuteContext<'_>) -> AsyncHostFuture {
    Box::pin(async { VMResult::Unreachable })
}

fn assert_reentrant_error(error: StoreExecutionError, api_name: &'static str) {
    assert_eq!(
        error.to_string(),
        format!(
            "{api_name} is unsupported while the same store execution is already active on this thread"
        )
    );
    assert!(matches!(
        error,
        StoreExecutionError::ReentrantCallDenied(actual_api_name) if actual_api_name == api_name
    ));
}

#[test]
fn run_module_function_with_driver_rejects_reentrant_store() {
    let (store, instance) = futures::executor::block_on(store_and_instance_for_test());
    let mut driver = TokioDriver::new();
    let _active = store.lock_runtime_unchecked();

    let result = futures::executor::block_on(vm::run_module_function_with_driver(
        &instance,
        &store,
        "run",
        &ResultValue::new(vec![]),
        &mut driver,
    ));

    assert!(matches!(result, VMResult::Unlinkable));
}

#[test]
fn get_global_rejects_reentrant_store() {
    let (store, instance) = futures::executor::block_on(store_and_instance_for_test());
    let _active = store.lock_runtime_unchecked();

    assert!(matches!(
        vm::get_global(&instance, &store, "g"),
        VMResult::Unlinkable
    ));
}

#[test]
fn instantiate_rejects_reentrant_store() {
    let module = parse_wat_for_test("(module)");
    let store = Store::new();
    let registry = Registry::new();
    let _active = store.lock_runtime_unchecked();

    assert!(matches!(
        futures::executor::block_on(instantiate::instantiate(module, &store, &registry)),
        VMResult::Unlinkable
    ));
}

#[test]
fn aliasing_rejects_reentrant_store() {
    let store = Store::new();
    let registry = Registry::new();
    let _active = store.lock_runtime_unchecked();

    assert!(matches!(
        instantiate::aliasing(&registry, &[], &store),
        VMResult::Unlinkable
    ));
}

#[test]
fn link_host_function_with_function_idx_impl_rejects_reentrant_store() {
    let (store, instance) = futures::executor::block_on(store_and_instance_for_test());
    let _active = store.lock_runtime_unchecked();

    assert_reentrant_error(
        link_host_function_with_function_idx_impl(&instance, 0, unreachable_host, &store)
            .expect_err("reentrant link must be rejected"),
        "link_host_function_with_function_idx",
    );
}

#[test]
fn link_host_function_with_export_name_impl_rejects_reentrant_store() {
    let (store, instance) = futures::executor::block_on(store_and_instance_for_test());
    let _active = store.lock_runtime_unchecked();

    assert_reentrant_error(
        link_host_function_with_export_name_impl(&instance, "run", unreachable_host, &store)
            .expect_err("reentrant link must be rejected"),
        "link_host_function_with_export_name",
    );
}

#[test]
fn link_async_host_function_with_function_idx_impl_rejects_reentrant_store() {
    let (store, instance) = futures::executor::block_on(store_and_instance_for_test());
    let _active = store.lock_runtime_unchecked();

    assert_reentrant_error(
        link_async_host_function_with_function_idx_impl(
            &instance,
            0,
            async_unreachable_host,
            &store,
        )
        .expect_err("reentrant link must be rejected"),
        "link_async_host_function_with_function_idx",
    );
}

#[test]
fn link_async_host_function_with_export_name_impl_rejects_reentrant_store() {
    let (store, instance) = futures::executor::block_on(store_and_instance_for_test());
    let _active = store.lock_runtime_unchecked();

    assert_reentrant_error(
        link_async_host_function_with_export_name_impl(
            &instance,
            "run",
            async_unreachable_host,
            &store,
        )
        .expect_err("reentrant link must be rejected"),
        "link_async_host_function_with_export_name",
    );
}

#[test]
fn take_last_trap_returns_none_while_store_is_active() {
    let store = Store::new();
    let _active = store.lock_runtime_unchecked();

    assert!(store.take_last_trap().is_none());
}
