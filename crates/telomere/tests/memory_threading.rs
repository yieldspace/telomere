mod common;

use common::instantiate_wat;
use telomere::{run_module_function, Registry, ResultValue, Store, VMResult, WasmValue};

#[cfg_attr(
    debug_assertions,
    ignore = "stack-sensitive regression is validated in release"
)]
#[tokio::test]
async fn release_memory_loop_keeps_tail_call_threading() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (func (export "run") (param $remaining i32) (result i32)
            i32.const 0
            i32.const 0
            i32.store
            block $done
              loop $loop
                local.get $remaining
                i32.eqz
                br_if $done

                i32.const 0
                i32.const 0
                i32.load
                i32.const 1
                i32.add
                i32.store

                local.get $remaining
                i32.const 1
                i32.sub
                local.set $remaining
                br $loop
              end
            end
            i32.const 0
            i32.load))
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
        other => panic!("memory loop must succeed, got {other:?}"),
    }
}

/// Stack size for the memory load/store loop test below (#285).
///
/// The loop runs 4,000,000 iterations, each executing an f32 load and
/// store through a local base pointer. Those handlers carried the #240
/// identity barrier, and with the barrier in its original `black_box`
/// form they lost their tail calls and left a live host frame per
/// execution, so on the unmodified tree this loop aborts with a host
/// stack overflow (measured: exit 134 on a 16 MiB thread). 16 MiB is
/// far above the flat stack the fixed dispatch uses, so the test is
/// red before the fix and green after it in both profiles (same
/// sizing argument as the `f64.const` loop test in #277 and the
/// `ref.is_null` loop test in #282). An i32 load/store loop does not
/// reproduce the leak: the i32 handlers are the canonical bodies the
/// f32/f64 twins fold into, so they never carried a barrier.
const MEMORY_LOOP_STACK_BYTES: usize = 16 * 1024 * 1024;

#[test]
fn memory_load_store_loop_keeps_direct_threading() {
    std::thread::Builder::new()
        .name("memory-load-store-loop-test".to_string())
        .stack_size(MEMORY_LOOP_STACK_BYTES)
        .spawn(|| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("building a runtime for the memory loop test");
            runtime.block_on(memory_loop_body());
        })
        .expect("spawning the memory loop test thread")
        .join()
        .expect("the memory loop test thread must not panic");
}

async fn memory_loop_body() {
    let store = Store::new();
    let registry = Registry::new();
    // Each iteration loads the f32 cell through a local base pointer,
    // adds 1, and stores it back, so the load/store handlers must run
    // on every iteration and the result is exactly the iteration
    // count (well below 2^24, so the f32 arithmetic is exact).
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (func (export "run") (param $iterations i32) (result f32)
            (local $index i32)
            (local $acc f32)
            (local $base i32)
            i32.const 4096
            local.set $base
            block $done
              loop $loop
                local.get $index
                local.get $iterations
                i32.ge_u
                br_if $done

                local.get $base
                f32.load offset=0
                f32.const 1
                f32.add
                local.set $acc

                local.get $base
                local.get $acc
                f32.store offset=0

                local.get $index
                i32.const 1
                i32.add
                local.set $index
                br $loop
              end
            end
            local.get $acc))
        "#,
        &store,
        &registry,
    )
    .await;

    let iterations = 4_000_000;
    let result = run_module_function(
        &instance,
        &store,
        "run",
        &ResultValue::new(vec![WasmValue::I32(iterations)]),
    )
    .await;

    match result {
        VMResult::Success(values) => {
            assert_eq!(
                values,
                ResultValue::new(vec![WasmValue::F32(iterations as f32)])
            );
        }
        other => panic!("memory load/store loop must succeed, got {other:?}"),
    }
}
