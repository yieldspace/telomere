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
///
/// This test is the both-profiles execution tripwire. It is not the
/// sensitive one: at 64 MiB it also passes on toolchains that break the
/// tail-call contract, because the leak those toolchains leave per call
/// cycle (measured at roughly 42-168 bytes on 1.97.0 and later) totals
/// only about 8.4-33.6 MiB over this loop. That is how the 1.97 break
/// went unnoticed after the move to this stack size. The release-side
/// sensitivity lives in `call_loop_keeps_direct_threading_on_a_small_stack`
/// below (#114).
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

/// Stack size for the small-stack tripwire below (#114).
///
/// The same 200,000-cycle body as `release_call_loop_keeps_direct_threading`,
/// but on a stack sized by the same discipline as the sibling tests in
/// this file (#277, #282): far above the flat usage of a build that keeps
/// the tail-call form, far below the total a leaking build consumes.
///
/// - Flat side: on the pinned 1.96.0 the loop completes in a 2 MiB stack
///   in release builds (measured 2026-11-27), so 4 MiB leaves at least
///   2x headroom.
/// - Leak side: on 1.97.0 and every later release measured (1.98.0,
///   1.99.1, 1.100.0) the loop leaves roughly 42-168 bytes of host stack
///   per call cycle, 8.4-33.6 MiB over the loop, so the smallest leak
///   total is still more than 2x this stack and the test aborts with a
///   stack overflow there.
///
/// History: when issue #114 was filed, this tripwire ran on the default
/// test thread (2 MiB class) and caught the 1.97 break. PR #250 (#154)
/// then moved it onto a 64 MiB thread so it could run in dev builds too,
/// which absorbed the leak and silently desensitised the release side;
/// the "recovery" later observed on 1.99/1.100 was that artifact, not a
/// codegen fix (re-measured 2026-11-27: the shipped 64 MiB shape passes
/// on every toolchain from 1.97.0 on, the same body overflows at 2 MiB
/// and 8 MiB on all of them).
const CALL_LOOP_SMALL_STACK_BYTES: usize = 4 * 1024 * 1024;

/// The #114 tail-call threading tripwire with its sensitivity restored.
///
/// Ignored by default and run explicitly in release builds: CI runs it
/// with `--ignored` in the workspace-test and release-size guard jobs,
/// and the toolchain watch runs this file with `--include-ignored`.
///
/// It cannot simply run everywhere. The contract surface is the release
/// build configuration (codegen-units = 1, thin LTO); the dev
/// configuration (many codegen units, no LTO) pays a per-cycle host
/// cost even on the pinned toolchain -- the same body overflows a
/// 16 MiB stack and needs 32 MiB in dev builds on 1.96.0 (measured
/// 2026-11-27) -- so no single stack size is both dev-passing and
/// release-sensitive at 200,000 iterations. The 64 MiB test above
/// remains the both-profiles execution coverage.
#[test]
#[ignore = "release-configuration tripwire for #114: run with --ignored in release builds (dev codegen pays a per-cycle host cost even on the pinned toolchain)"]
fn call_loop_keeps_direct_threading_on_a_small_stack() {
    std::thread::Builder::new()
        .name("call-loop-small-stack-test".to_string())
        .stack_size(CALL_LOOP_SMALL_STACK_BYTES)
        .spawn(|| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("building a runtime for the small-stack call loop test");
            runtime.block_on(call_loop_body());
        })
        .expect("spawning the small-stack call loop test thread")
        .join()
        .expect("the small-stack call loop test thread must not panic");
}

/// The #114 loop-dispatch fallback, exercised on the tripwire's shape.
///
/// This test exists only in `loop-dispatch-fallback` builds. It runs
/// the same 200,000-cycle body as the small-stack tripwire above on
/// the same 4 MiB stack. On toolchains that break the tail-call
/// contract (measured: 1.97.0 and every later release measured so
/// far), the threaded build aborts in this shape -- that is what the
/// tripwire detects -- while a fallback build completes it, because
/// the trampoline driver dispatches one handler per loop iteration
/// from a single frame and host-stack depth no longer grows with the
/// number of executed instructions. On the pinned toolchain both
/// builds complete the shape; the fallback's cost is measured
/// separately (see docs/core/optimizer.md).
#[cfg(feature = "loop-dispatch-fallback")]
#[test]
fn call_loop_completes_on_a_small_stack_with_loop_dispatch_fallback() {
    std::thread::Builder::new()
        .name("call-loop-fallback-test".to_string())
        .stack_size(CALL_LOOP_SMALL_STACK_BYTES)
        .spawn(|| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("building a runtime for the fallback call loop test");
            runtime.block_on(call_loop_body());
        })
        .expect("spawning the fallback call loop test thread")
        .join()
        .expect("the fallback call loop test thread must not panic");
}

/// The #114 loop-dispatch fallback in the lazy-JIT-call shape: an
/// interpreted loop calling a *compiled* callee (Codex review on
/// PR #387).
///
/// The shape is forced with a 1,200-byte JIT code-cache cap: the tiny
/// callee compiles (516 bytes emitted on x86_64) and stays resident,
/// while the caller -- whose loop body carries value-neutral filler
/// churn so its compiled form is ~3,000 bytes on x86_64 -- is rejected
/// as too large and stays interpreted. Every call in the loop then
/// crosses `enter_current_frame_from_lazy_call`: the callee runs
/// natively and its continuation is dispatched on the interpreter
/// side. The cache-stat assertions below pin that this really is the
/// configuration running (caller rejected, exactly the callee
/// resident), so the test cannot silently degenerate into a plain
/// interpreted or fully native loop.
///
/// The fallback's first version routed the lazy-call continuation
/// through `resume_loop_dispatch`, starting a second trampoline
/// driver nested inside the driver that dispatched the call handler.
/// One nesting level stayed on the host stack per call cycle, so
/// stack depth grew with the number of cycles again -- the exact
/// failure the fallback exists to remove. Measured on the pinned
/// toolchain with an instrumented driver (temporary probe, not
/// committed): the nesting depth reached the iteration count
/// (20,000) and the stack pointer descended ~5.76 MB over the loop;
/// after the fix (the lazy-call entry returns `call_code`'s bounce
/// to the enclosing driver) the depth stays at 1 and the descent is
/// zero, and 200,000 cycles complete on a 2 MiB stack.
///
/// A completion assertion alone cannot trip on the old behaviour --
/// the native-entry stack floor declines native entry as the stack
/// runs low, which unwinds the nesting and lets even the old build
/// finish on small stacks (with the compiled callee demoted to the
/// interpreter for stretches). What this test pins is the shape and
/// the continuation's correctness through the bounce-to-driver path:
/// a wrong `ctx.cont`, a lost bounce, or a double dispatch in that
/// hand-off fails the result or the cache-stat assertions here.
#[cfg(all(
    feature = "loop-dispatch-fallback",
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
#[test]
fn interpreted_loop_calling_compiled_callee_completes_on_a_small_stack_with_loop_dispatch_fallback()
{
    std::thread::Builder::new()
        .name("lazy-call-fallback-test".to_string())
        .stack_size(CALL_LOOP_SMALL_STACK_BYTES)
        .spawn(|| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("building a runtime for the lazy-call fallback test");
            runtime.block_on(lazy_call_loop_body());
        })
        .expect("spawning the lazy-call fallback test thread")
        .join()
        .expect("the lazy-call fallback test thread must not panic");
}

#[cfg(all(
    feature = "loop-dispatch-fallback",
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
async fn lazy_call_loop_body() {
    // Value-neutral filler churned on a scratch local, repeated so the
    // caller's compiled form is several times the code-cache cap below
    // on every supported target, while the callee stays tiny.
    let filler = r#"
        local.get $scratch
        i32.const 3
        i32.mul
        i32.const 7
        i32.xor
        i32.const 1
        i32.add
        local.set $scratch
        local.get $scratch
        i32.const 5
        i32.mul
        i32.const 11
        i32.xor
        i32.const 2
        i32.add
        local.set $scratch
"#;
    let wat = format!(
        r#"
        (module
          (func $step (param i32) (result i32)
            local.get 0
            i32.const 1
            i32.add)
          (func (export "run") (param $remaining i32) (result i32)
            (local $acc i32)
            (local $scratch i32)
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
                local.set $scratch
        {filler}
                local.get $remaining
                i32.const 1
                i32.sub
                local.set $remaining
                br $loop
              end
            end
            local.get $acc))
        "#,
        filler = filler.repeat(16)
    );

    const CODE_CACHE_CAP: u32 = 1_200;
    let mut runtime_config = telomere::RuntimeConfig::default();
    runtime_config.jit = telomere::JitConfig { enabled: true };
    runtime_config.limits.max_code_cache_bytes = CODE_CACHE_CAP;
    let store = Store::new_with_runtime_config(runtime_config);
    let registry = Registry::new();
    let instance = instantiate_wat(&wat, &store, &registry).await;

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
        other => panic!("lazy-call loop must succeed, got {other:?}"),
    }

    // Pin the shape: the caller was rejected as too large for the
    // cache (so the loop ran interpreted) and exactly the tiny callee
    // is resident as compiled code.
    let stats = store.jit_cache_stats();
    assert!(
        stats.rejected_functions >= 1,
        "the caller must be rejected as too large for the {CODE_CACHE_CAP}-byte          code cache, stats={stats:?}"
    );
    assert_eq!(
        stats.compiled_functions, 1,
        "exactly the callee must be resident as compiled code, stats={stats:?}"
    );
    assert!(
        stats.used_bytes <= CODE_CACHE_CAP as usize,
        "resident code must respect the cache cap, stats={stats:?}"
    );
}

/// Stack size for the ref.is_null loop test below (#282).
///
/// The loop runs 2,000,000 iterations. With the #240 identity barrier in
/// its original `black_box` form, `op_ref_is_null` lost its tail call and
/// left a ~32-byte host frame per execution, so this loop needed roughly
/// 64 MiB of host stack and aborted a normal thread. 16 MiB is far below
/// that leak and far above the flat stack the fixed dispatch uses, so
/// the test is red before the fix and green after it in both profiles
/// (same sizing argument as the `f64.const` loop test in #277).
const REF_IS_NULL_LOOP_STACK_BYTES: usize = 16 * 1024 * 1024;

#[test]
fn ref_is_null_loop_keeps_direct_threading() {
    std::thread::Builder::new()
        .name("ref-is-null-loop-test".to_string())
        .stack_size(REF_IS_NULL_LOOP_STACK_BYTES)
        .spawn(|| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("building a runtime for the ref.is_null loop test");
            runtime.block_on(ref_is_null_loop_body());
        })
        .expect("spawning the ref.is_null loop test thread")
        .join()
        .expect("the ref.is_null loop test thread must not panic");
}

async fn ref_is_null_loop_body() {
    let store = Store::new();
    let registry = Registry::new();
    // The reference under test alternates between `ref.null func` and a
    // real `ref.func` by index parity, so the optimizer cannot fold the
    // `ref.is_null` result to a constant: the handler itself must run on
    // every iteration. The accumulator counts the null (even) indices.
    let instance = instantiate_wat(
        r#"
        (module
          (func $target)
          (elem declare func $target)
          (func (export "run") (param $iterations i32) (result i32)
            (local $index i32)
            (local $acc i32)
            (local $ref funcref)
            block $done
              loop $loop
                local.get $index
                local.get $iterations
                i32.ge_u
                br_if $done

                local.get $index
                i32.const 1
                i32.and
                if
                  ref.func $target
                  local.set $ref
                else
                  ref.null func
                  local.set $ref
                end

                local.get $ref
                ref.is_null
                local.get $acc
                i32.add
                local.set $acc

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
            // Every even index in [0, iterations) takes the null branch.
            assert_eq!(
                values,
                ResultValue::new(vec![WasmValue::I32(iterations / 2)])
            );
        }
        other => panic!("ref.is_null loop must succeed, got {other:?}"),
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
