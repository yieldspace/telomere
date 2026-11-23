mod common;

use common::instantiate_wat;
use futures::executor::block_on;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Barrier, Mutex,
    },
    thread,
};
use telomere::{
    component_support::runtime::run_core_export_sync_reentrant, instantiate,
    link_host_function_with_function_idx, run_module_function, HostContext, InstanceHandle,
    IoReadBinaryReader, Registry, ResultValue, RuntimeConfig, Store, StoreState, TrapFrameKind,
    TrapInfo, TrapKind, VMResult, WasmParser, WasmValue,
};

#[cfg(feature = "jit")]
use telomere::JitConfig;

fn parse_wat(wat: &str) -> telomere::Module {
    let bytes = wat::parse_str(wat).expect("test WAT must parse");
    let mut reader = IoReadBinaryReader::from(bytes.as_slice());
    WasmParser::new(&mut reader)
        .parse_module()
        .expect("test module must parse")
}

async fn run_trap(wat: &str, export: &str, store: &Store) -> TrapInfo {
    let registry = Registry::new();
    let instance = instantiate_wat(wat, store, &registry).await;
    let result = run_module_function(&instance, store, export, &ResultValue::new(vec![])).await;
    assert!(matches!(result, VMResult::Unreachable));
    store
        .take_last_trap()
        .expect("trapping guest call must leave public trap information")
}

/// Like `run_trap`, but for traps whose public `VMResult` is not
/// `Unreachable` (div-by-zero, call_indirect mismatch, ...).
#[cfg(feature = "jit")]
async fn run_any_trap(wat: &str, export: &str, store: &Store) -> TrapInfo {
    let registry = Registry::new();
    let instance = instantiate_wat(wat, store, &registry).await;
    let result = run_module_function(&instance, store, export, &ResultValue::new(vec![])).await;
    assert!(
        !matches!(result, VMResult::Success(_)),
        "export {export} must trap, got {result:?}"
    );
    store
        .take_last_trap()
        .expect("trapping guest call must leave public trap information")
}

#[tokio::test]
async fn deep_symbolized_trap_is_consumed_and_success_clears_it() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module $deep
          (func $run (export "run") call $f1)
          (func $f1 call $f2)
          (func $f2 call $f3)
          (func $f3 unreachable)
          (func (export "ok")))
        "#,
        &store,
        &registry,
    )
    .await;

    let trapped = run_module_function(&instance, &store, "run", &ResultValue::new(vec![])).await;
    assert!(matches!(trapped, VMResult::Unreachable));

    let info = store
        .take_last_trap()
        .expect("the deep trap must be retrievable once");
    assert_eq!(info.kind, TrapKind::Unreachable);
    assert!(!info.truncated);
    assert_eq!(info.total_frames, 4);
    assert_eq!(info.frames.len(), 4);
    assert_eq!(
        info.frames
            .iter()
            .map(|frame| frame.depth)
            .collect::<Vec<_>>(),
        vec![0, 1, 2, 3]
    );
    assert_eq!(
        info.frames
            .iter()
            .map(|frame| frame.funcidx)
            .collect::<Vec<_>>(),
        vec![Some(3), Some(2), Some(1), Some(0)]
    );
    assert_eq!(
        info.frames
            .iter()
            .map(|frame| frame.func_name.as_deref())
            .collect::<Vec<_>>(),
        vec![Some("f3"), Some("f2"), Some("f1"), Some("run")]
    );
    assert_eq!(
        info.frames
            .iter()
            .map(|frame| frame.module_name.as_deref())
            .collect::<Vec<_>>(),
        vec![Some("deep"), Some("deep"), Some("deep"), Some("deep")]
    );
    assert!(info
        .frames
        .iter()
        .all(|frame| { frame.pc_index.is_some() && frame.kind == TrapFrameKind::Wasm }));
    assert!(
        store.take_last_trap().is_none(),
        "taking trap information must consume it"
    );

    let succeeded = run_module_function(&instance, &store, "ok", &ResultValue::new(vec![])).await;
    assert!(matches!(succeeded, VMResult::Success(_)));
    assert!(
        store.take_last_trap().is_none(),
        "a later successful outermost guest call must clear prior trap information"
    );
}

#[tokio::test]
async fn symbolization_uses_the_imports_first_function_index_space() {
    let store = Store::new();
    let mut registry = Registry::new();
    let imported = instantiate_wat(
        r#"
        (module
          (func (export "imported")))
        "#,
        &store,
        &registry,
    )
    .await;
    registry.register("host", imported);
    let instance = instantiate_wat(
        r#"
        (module $imports_first
          (import "host" "imported" (func $imported))
          (func $defined (export "run") unreachable))
        "#,
        &store,
        &registry,
    )
    .await;

    let result = run_module_function(&instance, &store, "run", &ResultValue::new(vec![])).await;
    assert!(matches!(result, VMResult::Unreachable));
    let info = store
        .take_last_trap()
        .expect("trapping call must be retained");
    let frame = info.frames.first().expect("trapping frame must exist");
    assert_eq!(frame.funcidx, Some(1));
    assert_eq!(frame.func_name.as_deref(), Some("defined"));
    assert_eq!(frame.module_name.as_deref(), Some("imports_first"));
}

#[tokio::test]
async fn traps_without_a_name_section_keep_indices_but_no_names() {
    let store = Store::new();
    let info = run_trap(
        r#"
        (module
          (func (export "run") unreachable))
        "#,
        "run",
        &store,
    )
    .await;

    let frame = info.frames.first().expect("trapping frame must exist");
    assert_eq!(frame.funcidx, Some(0));
    assert_eq!(frame.func_name, None);
    assert_eq!(frame.module_name, None);
    assert_eq!(frame.kind, TrapFrameKind::Wasm);
}

#[derive(Clone, Copy)]
enum ReentryMode {
    Recover,
    Propagate,
    ProbeActiveCallGuard,
}

struct ReentryState {
    instance: Mutex<Option<InstanceHandle>>,
    mode: ReentryMode,
    observed_none_while_active: AtomicBool,
}

fn return_from_host(_cx: &mut HostContext<'_, '_>) -> VMResult<Vec<WasmValue>> {
    VMResult::Success(vec![])
}

fn reentering_host(cx: &mut HostContext<'_, '_>) -> VMResult<Vec<WasmValue>> {
    let state = unsafe { cx.store().state.get::<ReentryState>() }
        .expect("reentry tests require ReentryState in StoreState");
    let instance = state
        .instance
        .lock()
        .expect("reentry instance mutex must not be poisoned")
        .clone()
        .expect("reentry instance must be installed before guest execution");
    let inner =
        run_core_export_sync_reentrant(&instance, cx.store(), "inner", &ResultValue::new(vec![]))
            .expect("synchronous reentry itself must be supported");
    assert!(matches!(inner, VMResult::Unreachable));

    match state.mode {
        ReentryMode::Recover => return_from_host(cx),
        ReentryMode::Propagate => VMResult::Unreachable,
        ReentryMode::ProbeActiveCallGuard => {
            state
                .observed_none_while_active
                .store(cx.store().take_last_trap().is_none(), Ordering::SeqCst);
            return_from_host(cx)
        }
    }
}

async fn reentry_fixture(mode: ReentryMode) -> (Store, InstanceHandle, &'static ReentryState) {
    let state = Box::leak(Box::new(ReentryState {
        instance: Mutex::new(None),
        mode,
        observed_none_while_active: AtomicBool::new(false),
    }));
    let store = Store::new_with_state(StoreState::from_static(state));
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module $reentry
          (func $host)
          (func $outer (export "outer") call $host)
          (func $inner (export "inner") unreachable))
        "#,
        &store,
        &registry,
    )
    .await;
    *state
        .instance
        .lock()
        .expect("reentry instance mutex must not be poisoned") = Some(instance.clone());
    link_host_function_with_function_idx(&instance, 0, reentering_host, &store);
    (store, instance, state)
}

#[tokio::test]
async fn recovered_nested_trap_does_not_escape_the_successful_outer_call() {
    let (store, instance, _) = reentry_fixture(ReentryMode::Recover).await;

    let result = run_module_function(&instance, &store, "outer", &ResultValue::new(vec![])).await;
    assert!(matches!(result, VMResult::Success(_)));
    assert!(
        store.take_last_trap().is_none(),
        "a recovered inner trap must be overwritten by outer success"
    );
}

#[tokio::test]
async fn propagated_nested_trap_reports_the_outer_chain_only() {
    let (store, instance, _) = reentry_fixture(ReentryMode::Propagate).await;

    let result = run_module_function(&instance, &store, "outer", &ResultValue::new(vec![])).await;
    assert!(matches!(result, VMResult::Unreachable));
    let info = store
        .take_last_trap()
        .expect("the outer failed call must publish its trap");
    assert_eq!(info.kind, TrapKind::Unreachable);
    assert_eq!(
        info.frames
            .iter()
            .map(|frame| frame.funcidx)
            .collect::<Vec<_>>(),
        vec![Some(0), Some(1)],
        "the separately stacked inner call is intentionally not stitched into the outer trace"
    );
    assert_eq!(
        info.frames
            .iter()
            .map(|frame| frame.kind)
            .collect::<Vec<_>>(),
        vec![TrapFrameKind::Host, TrapFrameKind::Wasm]
    );
    assert!(
        info.frames.iter().all(|frame| frame.funcidx != Some(2)),
        "the inner guest frame belongs to the independent reentry stack"
    );
}

#[tokio::test]
async fn display_with_names_is_a_stable_golden_format() {
    let store = Store::new();
    let info = run_trap(
        r#"
        (module $display
          (func $run (export "run") call $trap)
          (func $trap unreachable))
        "#,
        "run",
        &store,
    )
    .await;

    assert_eq!(
        info.to_string(),
        "trap: unreachable\n  0: display::trap (func 1) @ pc 0\n  1: display::run (func 0) @ pc 2"
    );
}

#[tokio::test]
async fn display_without_retained_names_uses_indices_in_the_same_golden_format() {
    let mut runtime_config = RuntimeConfig::default();
    runtime_config.diagnostics.retain_function_names = false;
    let store = Store::new_with_runtime_config(runtime_config);
    let info = run_trap(
        r#"
        (module $display
          (func $run (export "run") call $trap)
          (func $trap unreachable))
        "#,
        "run",
        &store,
    )
    .await;

    assert!(info.frames.iter().all(|frame| {
        frame.funcidx.is_some() && frame.func_name.is_none() && frame.module_name.is_none()
    }));
    assert_eq!(
        info.to_string(),
        "trap: unreachable\n  0: <unnamed> (func 1) @ pc 0\n  1: <unnamed> (func 0) @ pc 2"
    );
}

#[cfg(feature = "jit")]
#[tokio::test]
async fn jit_trap_keeps_caller_pcs_and_golden_frame_sequence() {
    if !telomere::jit_supported() {
        return;
    }

    let mut runtime_config = RuntimeConfig::default();
    runtime_config.jit = JitConfig { enabled: true };
    let store = Store::new_with_runtime_config(runtime_config);
    let info = run_trap(
        r#"
        (module $jit
          (func $run (export "run") call $f1)
          (func $f1 call $f2)
          (func $f2 unreachable))
        "#,
        "run",
        &store,
    )
    .await;

    assert_eq!(
        info.frames
            .iter()
            .map(|frame| (frame.funcidx, frame.func_name.as_deref()))
            .collect::<Vec<_>>(),
        vec![
            (Some(2), Some("f2")),
            (Some(1), Some("f1")),
            (Some(0), Some("run"))
        ]
    );
    // #209: the trapping frame now reports the `unreachable` site's pc
    // (index 0 in f2's decoded stream), matching the interpreter.
    assert_eq!(info.frames[0].pc_index, Some(0));
    assert!(
        info.frames[1..]
            .iter()
            .all(|frame| frame.pc_index.is_some()),
        "JIT callers retain decoded-stream return PCs"
    );
    assert_eq!(
        info.to_string(),
        "trap: unreachable\n  0: jit::f2 (func 2) @ pc 0\n  1: jit::f1 (func 1) @ pc 2\n  2: jit::run (func 0) @ pc 2"
    );
}

/// #209 differential: one compiled function with a div-by-zero, an
/// out-of-bounds load, and an `unreachable` at three distinct sites. For
/// each selector the JIT's innermost pc and trap kind must equal the
/// interpreter's exactly, and the three sites must yield three distinct
/// pcs.
#[cfg(feature = "jit")]
#[tokio::test]
async fn jit_trap_sites_match_interpreter_at_distinct_sites_in_one_function() {
    if !telomere::jit_supported() {
        return;
    }
    const WAT: &str = r#"
        (module $sites
          (memory 1)
          (func $run (export "run") (param i32) (result i32)
            local.get 0
            i32.eqz
            if (result i32)
              i32.const 1
              i32.const 0
              i32.div_s
            else
              local.get 0
              i32.const 1
              i32.eq
              if (result i32)
                i32.const 65536
                i32.load
              else
                unreachable
              end
            end))
        "#;

    async fn run_with(store: &Store, selector: i32) -> TrapInfo {
        let registry = Registry::new();
        let instance = instantiate_wat(WAT, store, &registry).await;
        let args = ResultValue::new(vec![telomere::WasmValue::I32(selector)]);
        let result = run_module_function(&instance, store, "run", &args).await;
        assert!(
            !matches!(result, VMResult::Success(_)),
            "selector {selector} must trap, got {result:?}"
        );
        store
            .take_last_trap()
            .expect("trapping guest call must leave public trap information")
    }

    let interpreter_store = Store::new();
    let mut runtime_config = RuntimeConfig::default();
    runtime_config.jit = JitConfig { enabled: true };
    let jit_store = Store::new_with_runtime_config(runtime_config);

    let mut pcs = Vec::new();
    for selector in [0, 1, 2] {
        let interp = run_with(&interpreter_store, selector).await;
        let jit = run_with(&jit_store, selector).await;
        assert_eq!(jit.kind, interp.kind, "selector {selector}: trap kind");
        assert_eq!(jit.frames.len(), 1, "selector {selector}: single frame");
        let interp_pc = interp.frames[0]
            .pc_index
            .expect("interpreter must attribute the trapping instruction");
        assert_eq!(
            jit.frames[0].pc_index,
            Some(interp_pc),
            "selector {selector}: JIT innermost pc must match the interpreter"
        );
        pcs.push(interp_pc);
    }
    assert!(
        pcs[0] != pcs[1] && pcs[1] != pcs[2] && pcs[0] != pcs[2],
        "the three trap sites must resolve to distinct pcs, got {pcs:?}"
    );
}

/// #209 differential: a trap in a nested JIT-compiled callee attributes
/// every frame — the innermost frame through its trap-site table, the
/// callers through their return pcs — exactly as the interpreter does.
#[cfg(feature = "jit")]
#[tokio::test]
async fn jit_nested_trap_attributes_every_frame_like_the_interpreter() {
    if !telomere::jit_supported() {
        return;
    }
    const WAT: &str = r#"
        (module $nested
          (func $run (export "run") call $mid)
          (func $mid call $leaf)
          (func $leaf i32.const 1 i32.const 0 i32.div_s drop))
        "#;

    let interpreter_store = Store::new();
    let interp = run_any_trap(WAT, "run", &interpreter_store).await;
    let mut runtime_config = RuntimeConfig::default();
    runtime_config.jit = JitConfig { enabled: true };
    let jit_store = Store::new_with_runtime_config(runtime_config);
    let jit = run_any_trap(WAT, "run", &jit_store).await;

    assert_eq!(jit.kind, interp.kind);
    assert_eq!(jit.frames.len(), interp.frames.len());
    for (jit_frame, interp_frame) in jit.frames.iter().zip(&interp.frames) {
        assert_eq!(jit_frame.funcidx, interp_frame.funcidx);
        assert_eq!(
            jit_frame.pc_index, interp_frame.pc_index,
            "frame {} must carry the same pc on both engines",
            jit_frame.depth
        );
        assert!(
            jit_frame.pc_index.is_some(),
            "frame {} must be attributed",
            jit_frame.depth
        );
    }
}

/// #209 fallback contract: a trap raised by a Rust stub (call_indirect
/// type mismatch) inside JIT-compiled code carries no trap site, so the
/// trapping frame reports no pc — never a wrong one — while the caller
/// frame keeps its return pc.
#[cfg(feature = "jit")]
#[tokio::test]
async fn jit_stub_trap_in_jit_caller_keeps_no_pc_but_caller_pc() {
    if !telomere::jit_supported() {
        return;
    }
    const WAT: &str = r#"
        (module $stub_trap
          (type $sig (func (result i32)))
          (type $other (func (result i64)))
          (table 1 funcref)
          (elem (i32.const 0) $callee)
          (func $callee (type $other) (result i64) i64.const 7)
          (func $run (export "run") (result i32)
            i32.const 0
            call_indirect (type $sig)))
        "#;

    let interpreter_store = Store::new();
    let interp = run_any_trap(WAT, "run", &interpreter_store).await;
    let mut runtime_config = RuntimeConfig::default();
    runtime_config.jit = JitConfig { enabled: true };
    let jit_store = Store::new_with_runtime_config(runtime_config);
    let jit = run_any_trap(WAT, "run", &jit_store).await;

    assert_eq!(jit.kind, interp.kind);
    assert_eq!(jit.frames.len(), interp.frames.len());
    assert_eq!(
        jit.frames[0].pc_index, None,
        "a stub-raised trap has no native site and must claim no pc"
    );
    assert!(
        interp.frames[0].pc_index.is_some(),
        "the interpreter attributes its own call_indirect"
    );
}

#[test]
fn concurrent_calls_never_read_another_threads_trap() {
    const ITERATIONS: usize = 64;

    let store = Arc::new(Store::new());
    let registry = Registry::new();
    let unreachable_instance = block_on(instantiate_wat(
        r#"
        (module
          (func (export "run") unreachable))
        "#,
        store.as_ref(),
        &registry,
    ));
    let out_of_bounds_instance = block_on(instantiate_wat(
        r#"
        (module
          (memory 1)
          (func (export "run") i32.const 65536 i32.load drop))
        "#,
        store.as_ref(),
        &registry,
    ));
    let after_trap = Arc::new(Barrier::new(2));
    let after_take = Arc::new(Barrier::new(2));

    let unreachable_worker = {
        let store = Arc::clone(&store);
        let instance = unreachable_instance.clone();
        let after_trap = Arc::clone(&after_trap);
        let after_take = Arc::clone(&after_take);
        thread::spawn(move || {
            let mut observed_wrong_kind = false;
            let mut execution_failed = false;
            for _ in 0..ITERATIONS {
                let result = block_on(run_module_function(
                    &instance,
                    store.as_ref(),
                    "run",
                    &ResultValue::new(vec![]),
                ));
                execution_failed |= !matches!(result, VMResult::Unreachable);
                after_trap.wait();
                if let Some(info) = store.take_last_trap() {
                    observed_wrong_kind |= info.kind != TrapKind::Unreachable;
                }
                after_take.wait();
            }
            (execution_failed, observed_wrong_kind)
        })
    };
    let out_of_bounds_worker = {
        let store = Arc::clone(&store);
        let instance = out_of_bounds_instance.clone();
        let after_trap = Arc::clone(&after_trap);
        let after_take = Arc::clone(&after_take);
        thread::spawn(move || {
            let mut observed_wrong_kind = false;
            let mut execution_failed = false;
            for _ in 0..ITERATIONS {
                let result = block_on(run_module_function(
                    &instance,
                    store.as_ref(),
                    "run",
                    &ResultValue::new(vec![]),
                ));
                execution_failed |= !matches!(result, VMResult::MemoryIndexOutOfRange);
                after_trap.wait();
                if let Some(info) = store.take_last_trap() {
                    observed_wrong_kind |= info.kind != TrapKind::MemoryIndexOutOfRange;
                }
                after_take.wait();
            }
            (execution_failed, observed_wrong_kind)
        })
    };

    let (unreachable_execution_failed, unreachable_observed_wrong_kind) = unreachable_worker
        .join()
        .expect("unreachable worker must finish without panic");
    let (out_of_bounds_execution_failed, out_of_bounds_observed_wrong_kind) = out_of_bounds_worker
        .join()
        .expect("out-of-bounds worker must finish without panic");
    assert!(!unreachable_execution_failed);
    assert!(!out_of_bounds_execution_failed);
    assert!(
        !unreachable_observed_wrong_kind,
        "an unreachable worker may lose its trap but must never read the other worker's trap"
    );
    assert!(
        !out_of_bounds_observed_wrong_kind,
        "an out-of-bounds worker may lose its trap but must never read the other worker's trap"
    );
}

#[tokio::test]
async fn trapping_start_function_is_reported_through_store_retrieval() {
    let store = Store::new();
    let registry = Registry::new();
    let result = instantiate(
        parse_wat(
            r#"
            (module $start_module
              (func $start unreachable)
              (start $start))
            "#,
        ),
        &store,
        &registry,
    )
    .await;
    assert!(matches!(result, VMResult::Unreachable));

    let info = store
        .take_last_trap()
        .expect("a trapping start function must publish trap information");
    assert_eq!(info.kind, TrapKind::Unreachable);
    assert_eq!(info.frames.len(), 1);
    assert_eq!(info.frames[0].funcidx, Some(0));
    assert_eq!(info.frames[0].func_name.as_deref(), Some("start"));
}

#[tokio::test]
async fn take_last_trap_returns_none_while_a_host_callback_is_active() {
    let (store, instance, state) = reentry_fixture(ReentryMode::ProbeActiveCallGuard).await;

    let result = run_module_function(&instance, &store, "outer", &ResultValue::new(vec![])).await;
    assert!(matches!(result, VMResult::Success(_)));
    assert!(
        state.observed_none_while_active.load(Ordering::SeqCst),
        "the inner trap is present while the host callback runs, so None proves the active-call guard"
    );
    assert!(
        store.take_last_trap().is_none(),
        "outer success clears the recovered inner trap after the callback returns"
    );
}
