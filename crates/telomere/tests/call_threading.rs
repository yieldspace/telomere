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
