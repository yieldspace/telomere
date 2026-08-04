mod common;

use std::{
    sync::{mpsc, Arc, Barrier, Mutex},
    time::Duration,
};

use common::instantiate_wat;
use telomere::{
    common::{ExecuteContext, FuncType, HostFunctionDefinition, Instr, NativeModule},
    run_module_function,
    runtime::instantiate_native_module,
    InterruptReason, MeteringConfig, Registry, ResultValue, RuntimeConfig, Store, StoreState,
    VMResult, WasmValue,
};

const LOOP_FOREVER_WAT: &str = r#"
    (module
      (func (export "spin")
        (loop $again
          br $again)))
"#;

const RETURN_CALL_FOREVER_WAT: &str = r#"
    (module
      (func $spin
        return_call $spin)
      (export "spin" (func $spin)))
"#;

const ORDINARY_DIRECT_CALL_WAT: &str = r#"
    (module
      (func $callee)
      (func (export "run")
        call $callee))
"#;

const ORDINARY_HOST_CALL_WAT: &str = r#"
    (module
      (import "gate" "noop" (func $noop))
      (func (export "run")
        call $noop))
"#;

const COUNTER_WAT: &str = r#"
    (module
      (func (export "count") (param i32) (result i32)
        (local $current i32)
        (block $done
          (loop $again
            local.get $current
            local.get 0
            i32.ge_u
            br_if $done
            local.get $current
            i32.const 1
            i32.add
            local.set $current
            br $again))
        local.get $current))
"#;

const NATIVE_MEMORY_BULK_WAT: &str = r#"
    (module
      (memory 1)
      (data (i32.const 0) "\a5")
      (data (i32.const 4096) "\5a")
      (data $payload "\cc")
      (func (export "copy")
        (memory.copy (i32.const 4096) (i32.const 0) (i32.const 8192)))
      (func (export "fill")
        (memory.fill (i32.const 0) (i32.const 0xaa) (i32.const 8192)))
      (func (export "fill_oob")
        (memory.fill (i32.const 65535) (i32.const 0xaa) (i32.const 8192)))
      (func (export "init")
        (memory.init $payload (i32.const 8192) (i32.const 0) (i32.const 1)))
      (func (export "fill_zero")
        (memory.fill (i32.const 0) (i32.const 0xff) (i32.const 0)))
      (func (export "copy_dst") (result i32)
        (i32.load8_u (i32.const 4096)))
      (func (export "fill_dst") (result i32)
        (i32.load8_u (i32.const 0)))
      (func (export "init_dst") (result i32)
        (i32.load8_u (i32.const 8192)))
      (func (export "zero_dst") (result i32)
        (i32.load8_u (i32.const 0))))
"#;

const MEMORY_GROW_WAT: &str = r#"
    (module
      (memory 1 2)
      (func (export "grow") (result i32)
        (memory.grow (i32.const 1)))
      (func (export "size") (result i32)
        memory.size))
"#;

const NATIVE_TABLE_BULK_WAT: &str = r#"
    (module
      (type $t (func))
      (func $value (type $t))
      (table 8192 12288 funcref)
      (elem (i32.const 0) func $value)
      (func (export "copy")
        (table.copy (i32.const 4096) (i32.const 0) (i32.const 4096)))
      (func (export "fill")
        (table.fill (i32.const 4096) (ref.func $value) (i32.const 4096)))
      (func (export "grow") (result i32)
        (table.grow (ref.null func) (i32.const 4096)))
      (func (export "grow_past_max") (result i32)
        (table.grow (ref.null func) (i32.const 8192)))
      (func (export "copy_dst_is_null") (result i32)
        (ref.is_null (table.get (i32.const 4096))))
      (func (export "fill_dst_is_null") (result i32)
        (ref.is_null (table.get (i32.const 5000))))
      (func (export "size") (result i32)
        table.size))
"#;

const INDEXED_MEMORY_COPY_WAT: &str = r#"
    (module
      (memory $src 1)
      (memory $dst 1)
      (func (export "same")
        (memory.copy $dst $dst (i32.const 4096) (i32.const 0) (i32.const 8192)))
      (func (export "cross")
        (memory.copy $dst $src (i32.const 0) (i32.const 0) (i32.const 8192))))
"#;

#[cfg(feature = "threads")]
const SHARED_NOTIFY_WAT: &str = r#"
    (module
      (memory 1 1 shared)
      (func (export "notify") (param i32) (result i32)
        (memory.atomic.notify (i32.const 0) (local.get 0))))
"#;

struct PauseState {
    entered: mpsc::SyncSender<()>,
    resume: Mutex<mpsc::Receiver<()>>,
}

fn pause_host(ctx: &mut ExecuteContext) -> VMResult<*const Instr> {
    let state =
        unsafe { ctx.store.state.get::<PauseState>() }.expect("test Store must retain PauseState");
    state
        .entered
        .send(())
        .expect("test controller must wait for the guest host call");
    state
        .resume
        .lock()
        .expect("test resume mutex must not be poisoned")
        .recv()
        .expect("test controller must release the host call");
    let (prev_local_ref, return_addr) = ctx.stack.function_return(&ctx.local_reference, 0, ctx.gc);
    ctx.set_local_reference(prev_local_ref);
    VMResult::Success(return_addr)
}

fn noop_host(ctx: &mut ExecuteContext) -> VMResult<*const Instr> {
    let (prev_local_ref, return_addr) = ctx.stack.function_return(&ctx.local_reference, 0, ctx.gc);
    ctx.set_local_reference(prev_local_ref);
    VMResult::Success(return_addr)
}

fn metered_store(initial_fuel: Option<u64>) -> Store {
    let mut runtime_config = RuntimeConfig::default();
    runtime_config.metering.enabled = true;
    runtime_config.metering.initial_fuel = initial_fuel;
    Store::new_with_runtime_config(runtime_config)
}

async fn run_noarg_wat(store: &Store, wat: &str, export: &str) -> VMResult<ResultValue> {
    let registry = Registry::new();
    let instance = instantiate_wat(wat, store, &registry).await;
    run_module_function(&instance, store, export, &ResultValue::new(vec![])).await
}

async fn run_counter(store: &Store, count: i32) -> VMResult<ResultValue> {
    let registry = Registry::new();
    let instance = instantiate_wat(COUNTER_WAT, store, &registry).await;
    run_module_function(
        &instance,
        store,
        "count",
        &ResultValue::new(vec![WasmValue::I32(count)]),
    )
    .await
}

fn assert_interrupted(result: VMResult<ResultValue>, expected: InterruptReason) {
    let actual = result.interrupt_reason();
    assert_eq!(
        actual,
        Some(expected),
        "expected interruption, got {result:?}"
    );
}

fn assert_counter_result(result: VMResult<ResultValue>, expected: i32) {
    match result {
        VMResult::Success(values) => {
            assert_eq!(values, ResultValue::new(vec![WasmValue::I32(expected)]))
        }
        other => panic!("expected successful counter result, got {other:?}"),
    }
}

#[tokio::test]
async fn metering_is_disabled_by_default() {
    assert!(!MeteringConfig::default().enabled);
    let default_store = Store::new();
    assert!(default_store.metering().is_none());
    assert_counter_result(run_counter(&default_store, 32).await, 32);

    let runtime_config = RuntimeConfig::default();
    assert!(!runtime_config.metering.enabled);
    assert!(Store::new_with_runtime_config(runtime_config)
        .metering()
        .is_none());
}

#[test]
fn metering_normalizes_a_requested_jit_off_without_the_jit_feature() {
    let mut runtime_config = RuntimeConfig::default();
    runtime_config.metering.enabled = true;
    runtime_config.jit.enabled = true;

    let store = Store::new_with_runtime_config(runtime_config);
    assert!(store.metering().is_some());
    assert!(
        !store.runtime_config().jit.enabled,
        "a metered Store must never retain a JIT-enabled effective configuration"
    );
}

#[tokio::test]
async fn fuel_exhaustion_stops_an_infinite_wasm_loop() {
    const FUEL: u64 = 64;

    let store = metered_store(Some(FUEL));
    let meter = store.metering().expect("metering must be enabled");
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        run_noarg_wat(&store, LOOP_FOREVER_WAT, "spin"),
    )
    .await
    .expect("fuel must bound the infinite loop");

    assert_interrupted(result, InterruptReason::FuelExhausted);
    assert_eq!(meter.fuel_remaining(), Some(0));
    assert_eq!(meter.fuel_consumed(), FUEL);
}

#[tokio::test]
async fn fuel_exhaustion_stops_return_call_self_recursion() {
    const FUEL: u64 = 64;

    let store = metered_store(Some(FUEL));
    let meter = store.metering().expect("metering must be enabled");
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        run_noarg_wat(&store, RETURN_CALL_FOREVER_WAT, "spin"),
    )
    .await
    .expect("fuel must bound tail-call recursion");

    assert_interrupted(result, InterruptReason::FuelExhausted);
    assert_eq!(meter.fuel_remaining(), Some(0));
    assert_eq!(meter.fuel_consumed(), FUEL);
}

#[tokio::test]
async fn ordinary_direct_and_host_calls_do_not_consume_fuel() {
    const FUEL: u64 = 7;

    let store = metered_store(Some(FUEL));
    let meter = store.metering().expect("metering must be enabled");

    assert!(matches!(
        run_noarg_wat(&store, ORDINARY_DIRECT_CALL_WAT, "run").await,
        VMResult::Success(_)
    ));
    assert_eq!(meter.fuel_consumed(), 0);
    assert_eq!(meter.fuel_remaining(), Some(FUEL));

    let mut registry = Registry::new();
    let host = match instantiate_native_module(
        NativeModule {
            functions: vec![HostFunctionDefinition {
                fp: noop_host,
                name: Some("noop".to_owned()),
                signature: FuncType::new(vec![], vec![]),
            }],
        },
        &store,
        &registry,
    )
    .await
    {
        VMResult::Success(host) => host,
        other => panic!("native noop module must instantiate, got {other:?}"),
    };
    registry.register("gate", host);

    let instance = instantiate_wat(ORDINARY_HOST_CALL_WAT, &store, &registry).await;
    assert!(matches!(
        run_module_function(&instance, &store, "run", &ResultValue::new(vec![])).await,
        VMResult::Success(_)
    ));
    assert_eq!(meter.fuel_consumed(), 0);
    assert_eq!(meter.fuel_remaining(), Some(FUEL));
}

#[tokio::test]
async fn cancellation_from_a_watchdog_thread_stops_guest_execution() {
    let store = metered_store(None);
    let meter = store.metering().expect("metering must be enabled");
    let registry = Registry::new();
    let instance = instantiate_wat(LOOP_FOREVER_WAT, &store, &registry).await;
    let watchdog_meter = meter.clone();
    let start_barrier = Arc::new(Barrier::new(3));
    let worker_barrier = Arc::clone(&start_barrier);
    let watchdog_barrier = Arc::clone(&start_barrier);
    let (result_tx, result_rx) = mpsc::sync_channel(1);

    let watchdog = std::thread::spawn(move || {
        watchdog_barrier.wait();
        watchdog_meter.interrupt();
    });
    let worker = std::thread::Builder::new()
        // Direct-threaded dispatch uses native frames in debug builds. A large stack makes this
        // timeout/cancellation integration test exercise the same unbounded guest path as release.
        .stack_size(64 * 1024 * 1024)
        .spawn(move || {
            worker_barrier.wait();
            let result = futures::executor::block_on(run_module_function(
                &instance,
                &store,
                "spin",
                &ResultValue::new(vec![]),
            ));
            result_tx
                .send(result)
                .expect("test receiver must remain alive until guest completion");
        })
        .expect("guest worker must start");

    // The worker cannot enter the guest before this barrier. The watchdog is a distinct OS
    // thread and requests cancellation immediately after the guest is released to run.
    start_barrier.wait();
    let result = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match result_rx.try_recv() {
                Ok(result) => return result,
                Err(mpsc::TryRecvError::Empty) => {
                    tokio::time::sleep(Duration::from_millis(1)).await
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    panic!("guest worker exited without sending a result")
                }
            }
        }
    })
    .await
    .expect("watchdog cancellation must return in bounded time");

    watchdog.join().expect("watchdog must not panic");
    worker.join().expect("guest worker must not panic");

    assert_interrupted(result, InterruptReason::Cancelled);
    assert!(meter.is_interrupted());
    meter.reset_interrupt();
    assert!(!meter.is_interrupted());
}

#[tokio::test]
async fn arithmetic_traps_are_distinct_from_metering_interrupts() {
    let store = metered_store(Some(64));
    let result = run_noarg_wat(
        &store,
        r#"
            (module
              (func (export "divide") (result i32)
                i32.const 1
                i32.const 0
                i32.div_s))
        "#,
        "divide",
    )
    .await;

    assert!(
        matches!(result, VMResult::InvalidOperand),
        "arithmetic trap must not be reported as an interruption: {result:?}"
    );
}

#[tokio::test]
async fn insufficient_native_memory_bulk_fuel_leaves_memory_unchanged() {
    for (operation, observation, expected, fuel) in [
        ("copy", "copy_dst", 0x5a, 2),
        ("fill", "fill_dst", 0xa5, 2),
        ("init", "init_dst", 0, 0),
    ] {
        let store = metered_store(Some(fuel));
        let meter = store.metering().expect("metering must be enabled");
        let registry = Registry::new();
        let instance = instantiate_wat(NATIVE_MEMORY_BULK_WAT, &store, &registry).await;

        assert_interrupted(
            run_module_function(&instance, &store, operation, &ResultValue::new(vec![])).await,
            InterruptReason::FuelExhausted,
        );
        assert_eq!(meter.fuel_remaining(), Some(0));
        assert_counter_result(
            run_module_function(&instance, &store, observation, &ResultValue::new(vec![])).await,
            expected,
        );
    }
}

#[tokio::test]
async fn native_memory_bulk_precharge_uses_the_full_length_and_charges_zero_length_once() {
    let store = metered_store(Some(3));
    let meter = store.metering().expect("metering must be enabled");
    let registry = Registry::new();
    let instance = instantiate_wat(NATIVE_MEMORY_BULK_WAT, &store, &registry).await;

    assert!(matches!(
        run_module_function(&instance, &store, "fill", &ResultValue::new(vec![])).await,
        VMResult::Success(_)
    ));
    assert_counter_result(
        run_module_function(&instance, &store, "fill_dst", &ResultValue::new(vec![])).await,
        0xaa,
    );
    assert_eq!(meter.fuel_remaining(), Some(0));
    assert_eq!(meter.fuel_consumed(), 3);

    let zero_store = metered_store(Some(1));
    let zero_meter = zero_store.metering().expect("metering must be enabled");
    let zero_registry = Registry::new();
    let zero_instance = instantiate_wat(NATIVE_MEMORY_BULK_WAT, &zero_store, &zero_registry).await;

    assert!(matches!(
        run_module_function(
            &zero_instance,
            &zero_store,
            "fill_zero",
            &ResultValue::new(vec![])
        )
        .await,
        VMResult::Success(_)
    ));
    assert_counter_result(
        run_module_function(
            &zero_instance,
            &zero_store,
            "zero_dst",
            &ResultValue::new(vec![]),
        )
        .await,
        0xa5,
    );
    assert_eq!(zero_meter.fuel_remaining(), Some(0));
    assert_eq!(zero_meter.fuel_consumed(), 1);
}

#[tokio::test]
async fn native_bulk_admission_charge_is_not_refunded_by_a_trap_or_limit_rejection() {
    let oob_store = metered_store(Some(3));
    let oob_meter = oob_store.metering().expect("metering must be enabled");
    let oob_registry = Registry::new();
    let oob_instance = instantiate_wat(NATIVE_MEMORY_BULK_WAT, &oob_store, &oob_registry).await;

    assert!(matches!(
        run_module_function(
            &oob_instance,
            &oob_store,
            "fill_oob",
            &ResultValue::new(vec![])
        )
        .await,
        VMResult::MemoryIndexOutOfRange
    ));
    assert_eq!(oob_meter.fuel_remaining(), Some(0));
    assert_eq!(oob_meter.fuel_consumed(), 3);

    let limit_store = metered_store(Some(9));
    let limit_meter = limit_store.metering().expect("metering must be enabled");
    let limit_registry = Registry::new();
    let limit_instance =
        instantiate_wat(NATIVE_TABLE_BULK_WAT, &limit_store, &limit_registry).await;

    assert_counter_result(
        run_module_function(
            &limit_instance,
            &limit_store,
            "grow_past_max",
            &ResultValue::new(vec![]),
        )
        .await,
        -1,
    );
    assert_counter_result(
        run_module_function(
            &limit_instance,
            &limit_store,
            "size",
            &ResultValue::new(vec![]),
        )
        .await,
        8192,
    );
    assert_eq!(limit_meter.fuel_remaining(), Some(0));
    assert_eq!(limit_meter.fuel_consumed(), 9);
}

#[cfg(feature = "threads")]
#[tokio::test]
async fn shared_atomic_notify_postcharges_wakes_not_the_requested_count() {
    let store = metered_store(Some(1));
    let meter = store.metering().expect("metering must be enabled");
    let registry = Registry::new();
    let instance = instantiate_wat(SHARED_NOTIFY_WAT, &store, &registry).await;

    assert_counter_result(
        run_module_function(
            &instance,
            &store,
            "notify",
            &ResultValue::new(vec![WasmValue::I32(-1)]),
        )
        .await,
        0,
    );
    assert_eq!(meter.fuel_remaining(), Some(0));
    assert_eq!(meter.fuel_consumed(), 1);
}

#[tokio::test]
async fn memory_grow_precharge_requires_the_full_page_cost_before_growth() {
    let insufficient_store = metered_store(Some(16));
    let insufficient_meter = insufficient_store
        .metering()
        .expect("metering must be enabled");
    let insufficient_registry = Registry::new();
    let insufficient_instance =
        instantiate_wat(MEMORY_GROW_WAT, &insufficient_store, &insufficient_registry).await;

    assert_interrupted(
        run_module_function(
            &insufficient_instance,
            &insufficient_store,
            "grow",
            &ResultValue::new(vec![]),
        )
        .await,
        InterruptReason::FuelExhausted,
    );
    assert_eq!(insufficient_meter.fuel_remaining(), Some(0));
    assert_counter_result(
        run_module_function(
            &insufficient_instance,
            &insufficient_store,
            "size",
            &ResultValue::new(vec![]),
        )
        .await,
        1,
    );

    let sufficient_store = metered_store(Some(17));
    let sufficient_meter = sufficient_store
        .metering()
        .expect("metering must be enabled");
    let sufficient_registry = Registry::new();
    let sufficient_instance =
        instantiate_wat(MEMORY_GROW_WAT, &sufficient_store, &sufficient_registry).await;

    assert_counter_result(
        run_module_function(
            &sufficient_instance,
            &sufficient_store,
            "grow",
            &ResultValue::new(vec![]),
        )
        .await,
        1,
    );
    assert_counter_result(
        run_module_function(
            &sufficient_instance,
            &sufficient_store,
            "size",
            &ResultValue::new(vec![]),
        )
        .await,
        2,
    );
    assert_eq!(sufficient_meter.fuel_remaining(), Some(0));
    assert_eq!(sufficient_meter.fuel_consumed(), 17);
}

#[tokio::test]
async fn native_table_bulk_precharge_is_atomic_and_uses_four_byte_elements() {
    for (operation, observation) in [("copy", "copy_dst_is_null"), ("fill", "fill_dst_is_null")] {
        let store = metered_store(Some(4));
        let meter = store.metering().expect("metering must be enabled");
        let registry = Registry::new();
        let instance = instantiate_wat(NATIVE_TABLE_BULK_WAT, &store, &registry).await;

        assert_interrupted(
            run_module_function(&instance, &store, operation, &ResultValue::new(vec![])).await,
            InterruptReason::FuelExhausted,
        );
        assert_eq!(meter.fuel_remaining(), Some(0));
        assert_counter_result(
            run_module_function(&instance, &store, observation, &ResultValue::new(vec![])).await,
            1,
        );
    }

    let copy_store = metered_store(Some(5));
    let copy_meter = copy_store.metering().expect("metering must be enabled");
    let copy_registry = Registry::new();
    let copy_instance = instantiate_wat(NATIVE_TABLE_BULK_WAT, &copy_store, &copy_registry).await;
    assert!(matches!(
        run_module_function(
            &copy_instance,
            &copy_store,
            "copy",
            &ResultValue::new(vec![])
        )
        .await,
        VMResult::Success(_)
    ));
    assert_counter_result(
        run_module_function(
            &copy_instance,
            &copy_store,
            "copy_dst_is_null",
            &ResultValue::new(vec![]),
        )
        .await,
        0,
    );
    assert_eq!(copy_meter.fuel_remaining(), Some(0));
    assert_eq!(copy_meter.fuel_consumed(), 5);

    let fill_store = metered_store(Some(5));
    let fill_meter = fill_store.metering().expect("metering must be enabled");
    let fill_registry = Registry::new();
    let fill_instance = instantiate_wat(NATIVE_TABLE_BULK_WAT, &fill_store, &fill_registry).await;
    assert!(matches!(
        run_module_function(
            &fill_instance,
            &fill_store,
            "fill",
            &ResultValue::new(vec![])
        )
        .await,
        VMResult::Success(_)
    ));
    assert_counter_result(
        run_module_function(
            &fill_instance,
            &fill_store,
            "fill_dst_is_null",
            &ResultValue::new(vec![]),
        )
        .await,
        0,
    );
    assert_eq!(fill_meter.fuel_remaining(), Some(0));
    assert_eq!(fill_meter.fuel_consumed(), 5);

    let grow_store = metered_store(Some(4));
    let grow_meter = grow_store.metering().expect("metering must be enabled");
    let grow_registry = Registry::new();
    let grow_instance = instantiate_wat(NATIVE_TABLE_BULK_WAT, &grow_store, &grow_registry).await;
    assert_interrupted(
        run_module_function(
            &grow_instance,
            &grow_store,
            "grow",
            &ResultValue::new(vec![]),
        )
        .await,
        InterruptReason::FuelExhausted,
    );
    assert_eq!(grow_meter.fuel_remaining(), Some(0));
    assert_counter_result(
        run_module_function(
            &grow_instance,
            &grow_store,
            "size",
            &ResultValue::new(vec![]),
        )
        .await,
        8192,
    );

    let sufficient_grow_store = metered_store(Some(5));
    let sufficient_grow_meter = sufficient_grow_store
        .metering()
        .expect("metering must be enabled");
    let sufficient_grow_registry = Registry::new();
    let sufficient_grow_instance = instantiate_wat(
        NATIVE_TABLE_BULK_WAT,
        &sufficient_grow_store,
        &sufficient_grow_registry,
    )
    .await;
    assert_counter_result(
        run_module_function(
            &sufficient_grow_instance,
            &sufficient_grow_store,
            "grow",
            &ResultValue::new(vec![]),
        )
        .await,
        8192,
    );
    assert_counter_result(
        run_module_function(
            &sufficient_grow_instance,
            &sufficient_grow_store,
            "size",
            &ResultValue::new(vec![]),
        )
        .await,
        12288,
    );
    assert_eq!(sufficient_grow_meter.fuel_remaining(), Some(0));
    assert_eq!(sufficient_grow_meter.fuel_consumed(), 5);
}

#[tokio::test]
async fn indexed_same_memory_copy_is_prepaid_but_cross_memory_copy_is_chunked() {
    let same_store = metered_store(Some(2));
    let same_meter = same_store.metering().expect("metering must be enabled");
    let same_registry = Registry::new();
    let same_instance = instantiate_wat(INDEXED_MEMORY_COPY_WAT, &same_store, &same_registry).await;
    assert_interrupted(
        run_module_function(
            &same_instance,
            &same_store,
            "same",
            &ResultValue::new(vec![]),
        )
        .await,
        InterruptReason::FuelExhausted,
    );
    assert_eq!(same_meter.fuel_remaining(), Some(0));

    let cross_store = metered_store(Some(2));
    let cross_meter = cross_store.metering().expect("metering must be enabled");
    let cross_registry = Registry::new();
    let cross_instance =
        instantiate_wat(INDEXED_MEMORY_COPY_WAT, &cross_store, &cross_registry).await;
    assert!(matches!(
        run_module_function(
            &cross_instance,
            &cross_store,
            "cross",
            &ResultValue::new(vec![])
        )
        .await,
        VMResult::Success(_)
    ));
    assert_eq!(cross_meter.fuel_remaining(), Some(0));
    assert_eq!(cross_meter.fuel_consumed(), 2);
}

#[tokio::test]
async fn fuel_exhaustion_is_deterministic_for_the_same_module_and_budget() {
    const FUEL: u64 = 97;

    async fn execute_once() -> (VMResult<ResultValue>, Option<u64>, u64) {
        let store = metered_store(Some(FUEL));
        let meter = store.metering().expect("metering must be enabled");
        let result = run_noarg_wat(&store, LOOP_FOREVER_WAT, "spin").await;
        (result, meter.fuel_remaining(), meter.fuel_consumed())
    }

    let first = execute_once().await;
    let second = execute_once().await;

    assert_interrupted(first.0, InterruptReason::FuelExhausted);
    assert_interrupted(second.0, InterruptReason::FuelExhausted);
    assert_eq!(first.1, Some(0));
    assert_eq!(second.1, Some(0));
    assert_eq!(first.2, FUEL);
    assert_eq!(second.2, FUEL);
}

#[tokio::test]
async fn unlimited_fuel_still_tracks_consumption_for_calibration() {
    let store = metered_store(None);
    let meter = store.metering().expect("metering must be enabled");
    assert_eq!(meter.fuel_remaining(), None);

    let before = meter.fuel_consumed();
    assert_counter_result(run_counter(&store, 32).await, 32);
    let after = meter.fuel_consumed();

    assert!(after > before, "finite guest work must charge checkpoints");
    assert_eq!(meter.fuel_remaining(), None);

    let max_store = metered_store(Some(u64::MAX));
    let max_meter = max_store.metering().expect("metering must be enabled");
    assert_eq!(
        max_meter.fuel_remaining(),
        None,
        "Some(u64::MAX) is the unlimited-fuel spelling"
    );
}

#[tokio::test]
async fn idle_release_and_set_fuel_preserve_accounting_without_resetting_consumed() {
    const INITIAL_FUEL: u64 = 10_000;
    const REPLACEMENT_FUEL: u64 = 5_000;

    let store = metered_store(Some(INITIAL_FUEL));
    let meter = store.metering().expect("metering must be enabled");
    let before = meter.fuel_consumed();

    assert_counter_result(run_counter(&store, 32).await, 32);
    let first_consumed = meter.fuel_consumed();
    let first_remaining = meter.fuel_remaining().expect("finite fuel expected");
    assert!(first_consumed > before);
    assert_eq!(INITIAL_FUEL - first_remaining, first_consumed - before);

    meter.set_fuel(REPLACEMENT_FUEL);
    assert_eq!(meter.fuel_remaining(), Some(REPLACEMENT_FUEL));
    assert_eq!(
        meter.fuel_consumed(),
        first_consumed,
        "set_fuel changes the limit, not the cumulative measurement"
    );

    assert_counter_result(run_counter(&store, 16).await, 16);
    let second_consumed = meter.fuel_consumed();
    let second_remaining = meter.fuel_remaining().expect("finite fuel expected");
    assert!(second_consumed > first_consumed);
    assert_eq!(
        REPLACEMENT_FUEL - second_remaining,
        second_consumed - first_consumed,
        "an idle release must return unused reservation before the next epoch"
    );
}

#[tokio::test]
async fn concurrent_set_fuel_preserves_an_ordinary_host_call_as_uncharged() {
    const INITIAL_FUEL: u64 = 10_000;
    const REPLACEMENT_FUEL: u64 = 17;

    let (entered_tx, entered_rx) = mpsc::sync_channel(1);
    let (resume_tx, resume_rx) = mpsc::sync_channel(1);
    let state = Box::leak(Box::new(PauseState {
        entered: entered_tx,
        resume: Mutex::new(resume_rx),
    }));

    let mut runtime_config = RuntimeConfig::default();
    runtime_config.metering.enabled = true;
    runtime_config.metering.initial_fuel = Some(INITIAL_FUEL);
    let store =
        Store::new_with_state_and_runtime_config(StoreState::from_static(state), runtime_config);
    let meter = store.metering().expect("metering must be enabled");
    let mut registry = Registry::new();
    let host = match instantiate_native_module(
        NativeModule {
            functions: vec![HostFunctionDefinition {
                fp: pause_host,
                name: Some("pause".to_owned()),
                signature: FuncType::new(vec![], vec![]),
            }],
        },
        &store,
        &registry,
    )
    .await
    {
        VMResult::Success(host) => host,
        other => panic!("native pause module must instantiate, got {other:?}"),
    };
    registry.register("gate", host);
    let instance = instantiate_wat(
        r#"
            (module
              (import "gate" "pause" (func $pause))
              (func (export "run")
                call $pause))
        "#,
        &store,
        &registry,
    )
    .await;

    let worker = std::thread::spawn(move || {
        futures::executor::block_on(run_module_function(
            &instance,
            &store,
            "run",
            &ResultValue::new(vec![]),
        ))
    });

    entered_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("guest must enter the host call before it is resumed");
    meter.set_fuel(REPLACEMENT_FUEL);
    resume_tx
        .send(())
        .expect("paused host call must be released");
    assert!(matches!(
        worker.join().expect("guest worker must not panic"),
        VMResult::Success(_)
    ));

    assert_eq!(
        meter.fuel_remaining(),
        Some(REPLACEMENT_FUEL),
        "unused fuel from the old epoch must not be refunded into the new limit"
    );
    assert_eq!(
        meter.fuel_consumed(),
        0,
        "an ordinary host call must not consume a metering checkpoint"
    );
}

#[tokio::test]
async fn fuel_exhaustion_wins_when_the_cancel_flag_is_already_set() {
    let store = metered_store(Some(0));
    let meter = store.metering().expect("metering must be enabled");
    meter.interrupt();

    let result = run_noarg_wat(&store, LOOP_FOREVER_WAT, "spin").await;
    assert_interrupted(result, InterruptReason::FuelExhausted);
}

#[cfg(feature = "jit")]
#[tokio::test]
async fn metered_store_normalizes_jit_off_and_does_not_use_its_cache() {
    use telomere::JitConfig;

    let mut runtime_config = RuntimeConfig::default();
    runtime_config.metering.enabled = true;
    runtime_config.metering.initial_fuel = Some(10_000);
    runtime_config.jit = JitConfig {
        enabled: true,
        ..JitConfig::default()
    };
    let store = Store::new_with_runtime_config(runtime_config);
    let meter = store.metering().expect("metering must remain enabled");
    assert!(!store.runtime_config().jit.enabled);

    let before = store.jit_cache_stats();
    assert_counter_result(run_counter(&store, 32).await, 32);
    let after = store.jit_cache_stats();
    assert_eq!(after.compiled_functions, before.compiled_functions);
    assert_eq!(after.rejected_functions, before.rejected_functions);
    assert!(meter.fuel_consumed() > 0);
}
