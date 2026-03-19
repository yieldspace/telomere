#![cfg(feature = "threads")]

mod common;

use common::instantiate_wat;
#[cfg(feature = "async-runtime")]
use telomere::common::AtomicWaitResult;
use telomere::{
    common::{AtomicRmwOp, SharedMemoryObject},
    run_module_function, Registry, ResultValue, Store, VMResult, WasmValue,
};

fn unwrap_success<T: std::fmt::Debug>(result: VMResult<T>) -> T {
    match result {
        VMResult::Success(value) => value,
        other => panic!("expected success, got {other:?}"),
    }
}

async fn call_i32(
    instance: &telomere::common::InstanceHandle,
    store: &Store,
    name: &str,
    args: Vec<WasmValue>,
) -> VMResult<i32> {
    match run_module_function(instance, store, name, &ResultValue::new(args)).await {
        VMResult::Success(values) => match values.iter().next() {
            Some(WasmValue::I32(value)) => VMResult::Success(*value),
            other => panic!("expected i32 result from {name}, got {other:?}"),
        },
        VMResult::Unreachable => VMResult::Unreachable,
        VMResult::StackOverflow => VMResult::StackOverflow,
        VMResult::MemoryIndexOutOfRange => VMResult::MemoryIndexOutOfRange,
        VMResult::TableIndexOutOfRange => VMResult::TableIndexOutOfRange,
        VMResult::CallIndirectInvalidType => VMResult::CallIndirectInvalidType,
        VMResult::TableUninitialized => VMResult::TableUninitialized,
        VMResult::Unlinkable => VMResult::Unlinkable,
        VMResult::InvalidOperand => VMResult::InvalidOperand,
        VMResult::UnalignedAtomic => VMResult::UnalignedAtomic,
    }
}

#[tokio::test]
async fn unshared_wait_traps_and_notify_returns_zero() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (func (export "notify") (param i32 i32) (result i32)
            local.get 0
            local.get 1
            memory.atomic.notify)
          (func (export "wait32") (param i32 i32 i64) (result i32)
            local.get 0
            local.get 1
            local.get 2
            memory.atomic.wait32))
        "#,
        &store,
        &registry,
    )
    .await;

    assert_eq!(
        unwrap_success(
            call_i32(
                &instance,
                &store,
                "notify",
                vec![WasmValue::I32(0), WasmValue::I32(3)],
            )
            .await,
        ),
        0
    );
    assert!(matches!(
        call_i32(
            &instance,
            &store,
            "wait32",
            vec![WasmValue::I32(0), WasmValue::I32(0), WasmValue::I64(0),],
        )
        .await,
        VMResult::InvalidOperand
    ));
}

#[cfg(feature = "async-runtime")]
#[tokio::test]
async fn shared_wait_notify_is_fifo_and_timeout_removes_waiter() {
    let shared = SharedMemoryObject::new(1, 1);
    unwrap_success(shared.atomic_store_u32(0, 7));

    assert!(matches!(
        unwrap_success(shared.register_wait32(0, 99)),
        AtomicWaitResult::NotEqual
    ));

    let first = match unwrap_success(shared.register_wait32(0, 7)) {
        AtomicWaitResult::Pending(wait) => wait,
        AtomicWaitResult::NotEqual => panic!("expected pending wait"),
    };
    let second = match unwrap_success(shared.register_wait32(0, 7)) {
        AtomicWaitResult::Pending(wait) => wait,
        AtomicWaitResult::NotEqual => panic!("expected pending wait"),
    };

    assert_eq!(unwrap_success(shared.notify_waiters(0, 1)), 1);
    assert_eq!(first.wait_result(shared.clone(), -1).await, 0);
    assert_eq!(second.wait_result(shared.clone(), 0).await, 2);
    assert_eq!(unwrap_success(shared.notify_waiters(0, 1)), 0);
}

#[test]
fn shared_atomic_rmw_cmpxchg_and_alignment_follow_contracts() {
    let shared = SharedMemoryObject::new(1, 1);
    unwrap_success(shared.atomic_store_u32(0, 0x1111_1111));

    assert_eq!(
        unwrap_success(shared.atomic_rmw_u32(0, AtomicRmwOp::Add, 1)),
        0x1111_1111
    );
    assert_eq!(unwrap_success(shared.atomic_load_u32(0)), 0x1111_1112);

    assert_eq!(
        unwrap_success(shared.atomic_cmpxchg_u32(0, 0x2222_2222, 0x3333_3333)),
        0x1111_1112
    );
    assert_eq!(unwrap_success(shared.atomic_load_u32(0)), 0x1111_1112);

    assert_eq!(
        unwrap_success(shared.atomic_cmpxchg_u32(0, 0x1111_1112, 0x3333_3333)),
        0x1111_1112
    );
    assert_eq!(unwrap_success(shared.atomic_load_u32(0)), 0x3333_3333);
    assert!(matches!(
        shared.atomic_load_u32(1),
        VMResult::UnalignedAtomic
    ));
}

#[tokio::test]
async fn misaligned_atomic_store_traps_without_partial_write() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (func (export "seed")
            i32.const 0
            i32.const 0x11223344
            i32.store)
          (func (export "misaligned_store")
            i32.const 1
            i32.const 0xaabbccdd
            i32.atomic.store)
          (func (export "load0") (result i32)
            i32.const 0
            i32.load))
        "#,
        &store,
        &registry,
    )
    .await;

    assert!(matches!(
        run_module_function(&instance, &store, "seed", &ResultValue::new(vec![])).await,
        VMResult::Success(_)
    ));
    assert!(matches!(
        run_module_function(
            &instance,
            &store,
            "misaligned_store",
            &ResultValue::new(vec![])
        )
        .await,
        VMResult::UnalignedAtomic
    ));
    assert_eq!(
        unwrap_success(call_i32(&instance, &store, "load0", vec![]).await),
        0x1122_3344
    );
}

#[cfg(feature = "multi-memory")]
#[tokio::test]
async fn indexed_shared_atomic_ops_use_nonzero_memidx() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (memory $m 1 2 shared)
          (func (export "seed") (param i32)
            (i32.atomic.store $m (i32.const 0) (local.get 0)))
          (func (export "load0") (result i32)
            (i32.atomic.load $m (i32.const 0)))
          (func (export "notify0") (result i32)
            (memory.atomic.notify $m (i32.const 0) (i32.const 1)))
          (func (export "wait_not_equal") (result i32)
            (memory.atomic.wait32 $m (i32.const 0) (i32.const 99) (i64.const 0))))
        "#,
        &store,
        &registry,
    )
    .await;

    assert!(matches!(
        run_module_function(
            &instance,
            &store,
            "seed",
            &ResultValue::new(vec![WasmValue::I32(7)]),
        )
        .await,
        VMResult::Success(_)
    ));
    assert_eq!(
        unwrap_success(call_i32(&instance, &store, "load0", vec![]).await),
        7
    );
    assert_eq!(
        unwrap_success(call_i32(&instance, &store, "notify0", vec![]).await),
        0
    );
    #[cfg(feature = "async-runtime")]
    assert_eq!(
        unwrap_success(call_i32(&instance, &store, "wait_not_equal", vec![]).await),
        1
    );
    #[cfg(not(feature = "async-runtime"))]
    assert!(matches!(
        call_i32(&instance, &store, "wait_not_equal", vec![]).await,
        VMResult::InvalidOperand
    ));
}
