mod common;

use common::instantiate_wat;
use telomere::{run_module_function, Registry, ResultValue, Store, VMResult, WasmValue};

/// Stack size for the thread this test runs on (#154).
///
/// In release builds the direct-threaded dispatch keeps a flat host stack
/// over the 200,000 call cycles below, so any normal thread would do. In
/// dev builds a small host cost per call cycle remains (measured: the loop
/// below needs between 8 MiB and 32 MiB there), so the test runs on an
/// explicit 64 MiB thread in both profiles instead of being ignored in
/// debug builds.
const CALL_LOOP_STACK_BYTES: usize = 64 * 1024 * 1024;

#[test]
fn release_call_loop_keeps_direct_threading() {
    std::thread::Builder::new()
        .name("call-loop-test".to_string())
        .stack_size(CALL_LOOP_STACK_BYTES)
        .spawn(|| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("building a runtime for the call loop test");
            runtime.block_on(call_loop_body());
        })
        .expect("spawning the call loop test thread")
        .join()
        .expect("the call loop test thread must not panic");
}

async fn call_loop_body() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (func $step (param i32) (result i32)
            local.get 0
            i32.const 1
            i32.add)
          (func (export "run") (param $remaining i32) (result i32)
            (local $acc i32)
            i32.const 0
            local.set $acc
            block $done
              loop $loop
                local.get $remaining
                i32.eqz
                br_if $done

                local.get $acc
                call $step
                local.set $acc

                local.get $remaining
                i32.const 1
                i32.sub
                local.set $remaining
                br $loop
              end
            end
            local.get $acc))
        "#,
        &store,
        &registry,
    )
    .await;

    let iterations = 200_000;
    let result = run_module_function(
        &instance,
        &store,
        "run",
        &ResultValue::new(vec![WasmValue::I32(iterations)]),
    )
    .await;

    match result {
        VMResult::Success(values) => {
            assert_eq!(values, ResultValue::new(vec![WasmValue::I32(iterations)]));
        }
        other => panic!("call loop must succeed, got {other:?}"),
    }
}

/// Stack size for the f64.const loop test below (#277).
///
/// The loop runs 2,000,000 iterations. With the #240 identity barrier in
/// its original `black_box` form, `op_f64_const` lost its tail call and
/// left a ~32-byte host frame per execution, so this loop needed roughly
/// 64 MiB of host stack and aborted a normal 8 MiB thread. 16 MiB is far
/// below that leak and far above the flat stack the fixed dispatch uses
/// (the loop completes in a 2 MiB stack in dev builds), so the test is
/// red before the fix and green after it in both profiles.
const F64_CONST_LOOP_STACK_BYTES: usize = 16 * 1024 * 1024;

#[test]
fn f64_const_loop_keeps_direct_threading() {
    std::thread::Builder::new()
        .name("f64-const-loop-test".to_string())
        .stack_size(F64_CONST_LOOP_STACK_BYTES)
        .spawn(|| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("building a runtime for the f64.const loop test");
            runtime.block_on(f64_const_loop_body());
        })
        .expect("spawning the f64.const loop test thread")
        .join()
        .expect("the f64.const loop test thread must not panic");
}

async fn f64_const_loop_body() {
    let store = Store::new();
    let registry = Registry::new();
    // The `f64-kernel` baseline corpus shape: two `f64.const` / `f64.add`
    // pairs per iteration, returning the iteration count exactly.
    let instance = instantiate_wat(
        r#"
        (module
          (func (export "run") (param $iterations i32) (result i32)
            (local $index i32)
            (local $accumulator f64)
            block $done
              loop $loop
                local.get $index
                local.get $iterations
                i32.ge_u
                br_if $done

                local.get $accumulator
                f64.const 0.25
                f64.add
                f64.const 0.75
                f64.add
                local.set $accumulator

                local.get $index
                i32.const 1
                i32.add
                local.set $index
                br $loop
              end
            end
            local.get $accumulator
            i32.trunc_f64_s))
        "#,
        &store,
        &registry,
    )
    .await;

    let iterations = 2_000_000;
    let result = run_module_function(
        &instance,
        &store,
        "run",
        &ResultValue::new(vec![WasmValue::I32(iterations)]),
    )
    .await;

    match result {
        VMResult::Success(values) => {
            assert_eq!(values, ResultValue::new(vec![WasmValue::I32(iterations)]));
        }
        other => panic!("f64.const loop must succeed, got {other:?}"),
    }
}
