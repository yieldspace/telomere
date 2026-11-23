//! Native-code execution support: the store cache / load path, runtime
//! stubs, the native-call ABI, and the interpreter entry bridges.
//!
//! In an `aot-load`-only build (no `jit` feature), the stub table, the
//! cache's install path, and the entry bridges are reachable only from
//! native code -- code an AoT loader installs -- and from the emitters that
//! would take their addresses, so Rust-side reachability reports them as
//! dead. They are the load ABI and must stay; the scoped allowance below
//! records that instead of deleting or feature-gating the surface away.
#![cfg_attr(all(feature = "aot-load", not(feature = "jit")), allow(dead_code))]

#[cfg(feature = "aot-load")]
mod abi;
#[cfg(all(test, feature = "jit"))]
mod artifact_measure;
#[cfg(feature = "jit")]
mod backend;
#[cfg(feature = "aot-load")]
mod cache;
#[cfg(feature = "jit")]
mod emit_env;
#[cfg(feature = "aot-load")]
pub(crate) mod profile;
#[cfg(all(test, feature = "jit"))]
mod reloc_tests;
#[cfg(feature = "aot-load")]
mod stubs;

#[cfg(feature = "aot-load")]
use crate::{
    common::{store::FunctionBody, ExecuteContext, Instr, VMResult},
    runtime::vm,
};
#[cfg(feature = "aot-load")]
use std::cell::Cell;
#[cfg(feature = "aot-load")]
use std::sync::atomic::{AtomicUsize, Ordering};

#[cfg(feature = "aot-load")]
thread_local! {
    static JIT_ENTRY_DEPTH: Cell<usize> = const { Cell::new(0) };
    static JIT_INTERPRETER_STOP_AT: Cell<*const Instr> = const { Cell::new(std::ptr::null()) };
}

#[cfg(feature = "aot-load")]
pub use abi::JitNativeExit;
#[cfg(feature = "aot-load")]
pub use cache::JitCacheStats;
#[cfg(feature = "aot-load")]
pub(crate) use cache::{CompiledFunction, StoreJitCache};

/// Returns whether this build can execute native code on the current
/// target: the cache / load path (the `aot-load` feature, implied by `jit`)
/// is compiled in and the target is one the native ABI supports.
///
/// This is the internal execution predicate. The public
/// [`crate::jit_supported`] is stricter: it additionally requires the `jit`
/// feature, because only that feature can *compile* code at runtime.
pub fn supported() -> bool {
    native_supported()
}

#[cfg(feature = "aot-load")]
fn native_supported() -> bool {
    telomere_jit_codegen::target::supported()
}

#[cfg(not(feature = "aot-load"))]
fn native_supported() -> bool {
    false
}

#[cfg(feature = "aot-load")]
pub(crate) unsafe fn enter_current_frame(ctx: &mut ExecuteContext<'_>) -> VMResult<()> {
    let _profile_guard = profile::RunGuard::new();
    let code_base = ctx.code();
    let exit = match unsafe { enter_current_frame_raw(ctx) } {
        VMResult::Success(exit) => exit,
        VMResult::Unimplemented => {
            ctx.cont = code_base;
            return VMResult::Success(());
        }
        VMResult::Unreachable => return VMResult::Unreachable,
        VMResult::StackOverflow => return VMResult::StackOverflow,
        VMResult::MemoryIndexOutOfRange => return VMResult::MemoryIndexOutOfRange,
        VMResult::TableIndexOutOfRange => return VMResult::TableIndexOutOfRange,
        VMResult::CallIndirectInvalidType => return VMResult::CallIndirectInvalidType,
        VMResult::TableUninitialized => return VMResult::TableUninitialized,
        VMResult::Unlinkable => return VMResult::Unlinkable,
        VMResult::MemoryAllocationFailed => return VMResult::MemoryAllocationFailed,
        VMResult::InvalidOperand => return VMResult::InvalidOperand,
        VMResult::DivideByZero => return VMResult::DivideByZero,
        VMResult::IntegerOverflow => return VMResult::IntegerOverflow,
        VMResult::InvalidConversion => return VMResult::InvalidConversion,
        VMResult::UnalignedAtomic => return VMResult::UnalignedAtomic,
        VMResult::FuelExhausted => return VMResult::FuelExhausted,
        VMResult::Cancelled => return VMResult::Cancelled,
    };
    unsafe { handle_exit(exit, code_base, ctx) }
}

#[cfg(feature = "aot-load")]
pub(crate) unsafe fn enter_current_frame_from_jit_call(
    ctx: &mut ExecuteContext<'_>,
) -> JitNativeExit {
    let code_base = ctx.code();
    match unsafe { enter_current_frame_raw(ctx) } {
        VMResult::Success(exit) => unsafe { absolutize_fallback_index(exit, code_base) },
        VMResult::Unimplemented => JitNativeExit::fallback_pc(code_base),
        other => JitNativeExit::trap(other),
    }
}

#[cfg(feature = "aot-load")]
pub(crate) unsafe fn enter_current_frame_from_lazy_call(
    ctx: &mut ExecuteContext<'_>,
    continuation: *const Instr,
) -> VMResult<()> {
    let code_base = ctx.code();
    let exit = match unsafe { enter_current_frame_raw(ctx) } {
        VMResult::Success(exit) => exit,
        VMResult::Unimplemented => {
            ctx.cont = code_base;
            return VMResult::Success(());
        }
        VMResult::Unreachable => return VMResult::Unreachable,
        VMResult::StackOverflow => return VMResult::StackOverflow,
        VMResult::MemoryIndexOutOfRange => return VMResult::MemoryIndexOutOfRange,
        VMResult::TableIndexOutOfRange => return VMResult::TableIndexOutOfRange,
        VMResult::CallIndirectInvalidType => return VMResult::CallIndirectInvalidType,
        VMResult::TableUninitialized => return VMResult::TableUninitialized,
        VMResult::Unlinkable => return VMResult::Unlinkable,
        VMResult::MemoryAllocationFailed => return VMResult::MemoryAllocationFailed,
        VMResult::InvalidOperand => return VMResult::InvalidOperand,
        VMResult::DivideByZero => return VMResult::DivideByZero,
        VMResult::IntegerOverflow => return VMResult::IntegerOverflow,
        VMResult::InvalidConversion => return VMResult::InvalidConversion,
        VMResult::UnalignedAtomic => return VMResult::UnalignedAtomic,
        VMResult::FuelExhausted => return VMResult::FuelExhausted,
        VMResult::Cancelled => return VMResult::Cancelled,
    };
    if exit.kind == JitNativeExit::CONTINUE_PTR && exit.value == continuation as u64 {
        return unsafe { vm::call_code(continuation, ctx) };
    }
    unsafe { handle_exit(exit, code_base, ctx) }
}

#[cfg(feature = "aot-load")]
pub(crate) unsafe fn run_interpreter_continue_from_jit_call(
    pc: *const crate::common::Instr,
    stop_pc: *const crate::common::Instr,
    ctx: &mut ExecuteContext<'_>,
) -> JitNativeExit {
    trace_fallback_exit(ctx, "nested_continue", pc);
    let _guard = JitInterpreterStopGuard::new(stop_pc);
    let mut next = pc;
    loop {
        match unsafe { vm::call_code(next, ctx) } {
            VMResult::Success(()) => {
                if ctx.cont.is_null() {
                    return JitNativeExit::done();
                }
                if ctx.cont == stop_pc {
                    return JitNativeExit::done();
                }
                next = ctx.cont;
            }
            VMResult::Unimplemented => return JitNativeExit::fallback_pc(ctx.cont),
            other => return JitNativeExit::trap(other),
        }
    }
}

#[cfg(feature = "aot-load")]
pub(crate) fn should_stop_interpreter_at(pc: *const Instr) -> bool {
    JIT_INTERPRETER_STOP_AT.with(|stop| {
        let target = stop.get();
        !target.is_null() && target == pc
    })
}

#[cfg(feature = "aot-load")]
pub(crate) fn interpreter_stop_active() -> bool {
    // Return-call and function-return handlers must leave the interpreter loop
    // whenever any bridge owns control. The loop then decides whether the
    // observed continuation is the requested stop PC or another VM PC to resume.
    JIT_INTERPRETER_STOP_AT.with(|stop| !stop.get().is_null())
}

#[cfg(feature = "aot-load")]
unsafe fn absolutize_fallback_index(
    exit: JitNativeExit,
    code_base: *const crate::common::Instr,
) -> JitNativeExit {
    if exit.kind == JitNativeExit::FALLBACK_INDEX {
        unsafe { JitNativeExit::fallback_pc(code_base.add(exit.value as usize)) }
    } else {
        exit
    }
}

#[cfg(feature = "aot-load")]
unsafe fn enter_current_frame_raw(ctx: &mut ExecuteContext<'_>) -> VMResult<JitNativeExit> {
    if !supported() || !ctx.store.runtime_config().jit.enabled {
        return VMResult::Unimplemented;
    }
    let funcaddr = ctx.current_frame.code_addr;
    if ctx.gc.jit_rejected_func(funcaddr) {
        profile::count(profile::Counter::CompileRejectKnownRejected);
        return VMResult::Unimplemented;
    }
    let too_deep = JIT_ENTRY_DEPTH.with(|depth| {
        let current = depth.get();
        if current >= 256 {
            if std::env::var_os("TELOMERE_JIT_TRACE_FALLBACK").is_some() {
                eprintln!("[telomere-jit] entry_depth_overflow current={current}");
            }
            return true;
        }
        depth.set(current.saturating_add(1));
        false
    });
    if too_deep {
        return VMResult::StackOverflow;
    }
    let _entry_guard = JitEntryDepthGuard;
    let (compiled, code_base) = {
        let func = ctx.gc.get_func(funcaddr);
        let FunctionBody::Wasm { code, .. } = &func.body else {
            return VMResult::Unimplemented;
        };
        let code_base = code.as_ptr();
        let compiled = match ctx.gc.jit_cached_compiled_func(funcaddr) {
            Some(compiled) if ctx.store.jit_cache().touch_compiled(funcaddr, &compiled) => {
                profile::count(profile::Counter::CompileFrameCacheHit);
                compiled
            }
            _ => {
                let compiled = vm_try!(load_or_compile(ctx.store, ctx.gc, funcaddr, &func.body));
                ctx.gc.set_jit_cached_compiled_func(funcaddr, &compiled);
                compiled
            }
        };
        (compiled, code_base)
    };
    let exit = unsafe {
        (compiled.entry())(
            ctx as *mut ExecuteContext<'_>,
            code_base,
            ctx.local_base_ptr,
        )
    };
    VMResult::Success(resolve_trap_site(exit, &compiled, code_base, ctx))
}

/// #209: resolves a native trap exit's site id against the trap-site
/// table of the function whose entry produced the exit.
///
/// This runs at the innermost JIT entry boundary for the trapping
/// function — the only place where the exit's function is unambiguous,
/// because trap exits propagate verbatim through native callers. The
/// resolved instruction pointer is stashed in `ctx.jit_trap_pc` (the
/// innermost resolution wins; outer boundaries see it already set) and
/// the exit value is normalised to the bare trap code, so no outer
/// layer can resolve the same site id against a different function's
/// table. A site that does not resolve degrades to no pc, never a
/// wrong one.
#[cfg(feature = "jit")]
fn resolve_trap_site(
    exit: JitNativeExit,
    compiled: &CompiledFunction,
    code_base: *const Instr,
    ctx: &mut ExecuteContext<'_>,
) -> JitNativeExit {
    if exit.kind != JitNativeExit::TRAP {
        return exit;
    }
    let (code, site) = abi::split_trap_value(exit.value);
    if site != 0 && ctx.jit_trap_pc.is_null() {
        if let Some(pc_index) = compiled.trap_pc(site) {
            // SAFETY: `pc_index` comes from `compiled`'s own trap-site
            // table, whose entries are pc indices into the decoded
            // stream starting at `code_base` — the stream of the very
            // function `compiled` was built from.
            ctx.jit_trap_pc = unsafe { code_base.add(pc_index as usize) };
        }
    }
    JitNativeExit {
        kind: exit.kind,
        value: code,
    }
}

/// Resolves native code for `funcaddr` after the per-frame cache missed.
///
/// With the `jit` feature this compiles the function on demand. Without it
/// (an `aot-load`-only build) the store cache is consulted and nothing else:
/// a function no AoT loader installed reports `VMResult::Unimplemented` --
/// the same typed result a JIT-rejected function produces -- and the caller
/// falls back to the interpreter. An `aot-load`-only build therefore cannot
/// compile at runtime, and the request surfaces as a typed result, never a
/// panic and never a silent compile.
#[cfg(feature = "jit")]
fn load_or_compile(
    store: &crate::Store,
    runtime: &crate::common::store::StoreInner,
    funcaddr: crate::common::ObjectRef,
    body: &FunctionBody,
) -> VMResult<std::sync::Arc<CompiledFunction>> {
    let max_bytes = store.runtime_config().jit.code_cache_max_bytes;
    store
        .jit_cache()
        .get_or_compile(funcaddr, body, runtime, max_bytes)
}

#[cfg(all(feature = "aot-load", not(feature = "jit")))]
fn load_or_compile(
    store: &crate::Store,
    _runtime: &crate::common::store::StoreInner,
    funcaddr: crate::common::ObjectRef,
    _body: &FunctionBody,
) -> VMResult<std::sync::Arc<CompiledFunction>> {
    profile::count(profile::Counter::CompileAttempt);
    match store.jit_cache().get_cached(funcaddr) {
        Some(compiled) => VMResult::Success(compiled),
        None => VMResult::Unimplemented,
    }
}

#[cfg(feature = "aot-load")]
struct JitEntryDepthGuard;

#[cfg(feature = "aot-load")]
struct JitInterpreterStopGuard {
    previous: *const Instr,
}

#[cfg(feature = "aot-load")]
impl JitInterpreterStopGuard {
    fn new(stop_pc: *const Instr) -> Self {
        // Continuation bridges can re-enter the interpreter while an outer bridge
        // is already active. The innermost native caller owns the active stop PC;
        // the previous target is restored when that bridge returns.
        let previous = JIT_INTERPRETER_STOP_AT.with(|stop| {
            let previous = stop.get();
            stop.set(stop_pc);
            previous
        });
        Self { previous }
    }
}

#[cfg(feature = "aot-load")]
impl Drop for JitEntryDepthGuard {
    fn drop(&mut self) {
        JIT_ENTRY_DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
    }
}

#[cfg(feature = "aot-load")]
impl Drop for JitInterpreterStopGuard {
    fn drop(&mut self) {
        JIT_INTERPRETER_STOP_AT.with(|stop| stop.set(self.previous));
    }
}

#[cfg(feature = "aot-load")]
unsafe fn handle_exit(
    exit: JitNativeExit,
    _code_base: *const crate::common::Instr,
    ctx: &mut ExecuteContext<'_>,
) -> VMResult<()> {
    match exit.kind {
        JitNativeExit::FALLBACK_INDEX => {
            profile::count_exit(exit.kind);
            VMResult::InvalidOperand
        }
        JitNativeExit::FALLBACK_PTR => unsafe {
            profile::count_exit(exit.kind);
            trace_fallback_exit(ctx, "fallback_ptr", exit.value as *const _);
            ctx.cont = exit.value as *const _;
            VMResult::Success(())
        },
        JitNativeExit::CONTINUE_PTR => {
            profile::count_exit(exit.kind);
            ctx.cont = exit.value as *const _;
            VMResult::Success(())
        }
        JitNativeExit::DONE => {
            profile::count_exit(exit.kind);
            ctx.cont = std::ptr::null();
            VMResult::Success(())
        }
        JitNativeExit::PENDING => {
            profile::count_exit(exit.kind);
            VMResult::Success(())
        }
        JitNativeExit::TRAP => {
            profile::count_exit(exit.kind);
            // #209: a trap site resolved at the trapping function's own
            // entry boundary was stashed in `jit_trap_pc`; it is the
            // only pc a native trap may claim. Site-less traps leave
            // `cont` null, so capture reports the trapping frame's pc
            // as unknown instead of trusting a stale continuation.
            // Exits reaching this point are already normalised by
            // `resolve_trap_site`; the split is defensive so a sited
            // value could never decode to a wrong VMResult either.
            ctx.cont = std::mem::replace(&mut ctx.jit_trap_pc, std::ptr::null());
            abi::vm_result_from_code(abi::split_trap_value(exit.value).0)
        }
        JitNativeExit::KEEP_GOING => {
            profile::count_exit(exit.kind);
            VMResult::InvalidOperand
        }
        _ => {
            profile::count_exit(exit.kind);
            VMResult::InvalidOperand
        }
    }
}

#[cfg(feature = "aot-load")]
unsafe fn trace_fallback_exit(
    ctx: &ExecuteContext<'_>,
    kind: &'static str,
    pc: *const crate::common::Instr,
) {
    static FALLBACK_TRACE_COUNT: AtomicUsize = AtomicUsize::new(0);
    if std::env::var_os("TELOMERE_JIT_TRACE_FALLBACK").is_none() {
        return;
    }
    let pc_index = unsafe { pc.offset_from(ctx.current_frame.code_base) };
    let funcidx = ctx
        .instance()
        .funcs
        .iter()
        .position(|addr| *addr == ctx.current_frame.code_addr)
        .map(|index| index.to_string())
        .unwrap_or_else(|| "unknown".to_owned());
    if let Ok(requested_funcidx) = std::env::var("TELOMERE_JIT_TRACE_FALLBACK_FUNC") {
        if funcidx != requested_funcidx {
            return;
        }
    }
    if let Ok(requested_kind) = std::env::var("TELOMERE_JIT_TRACE_FALLBACK_KIND") {
        if kind != requested_kind {
            return;
        }
    }
    let max = std::env::var("TELOMERE_JIT_TRACE_FALLBACK_MAX")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(usize::MAX);
    let index = FALLBACK_TRACE_COUNT.fetch_add(1, Ordering::Relaxed);
    if index >= max {
        return;
    }
    eprintln!("[telomere-jit] {kind} funcidx={funcidx} pc={pc_index}");
}

#[cfg(all(test, feature = "aot-load", not(feature = "jit")))]
mod aot_load_tests {
    use crate::{
        instantiate, run_module_function, InstanceHandle, IoReadBinaryReader, Registry,
        ResultValue, RuntimeConfig, Store, VMResult, WasmParser, WasmValue,
    };

    fn parse_wat(wat_src: &str) -> crate::Module {
        let bytes = wat::parse_str(wat_src).expect("test WAT must parse");
        let mut reader = IoReadBinaryReader::from(bytes.as_slice());
        WasmParser::new(&mut reader)
            .parse_module()
            .expect("test module must parse")
    }

    async fn instantiate_wat(store: &Store, wat_src: &str) -> InstanceHandle {
        let registry = Registry::new();
        match instantiate(parse_wat(wat_src), store, &registry).await {
            VMResult::Success(instance) => instance,
            other => panic!("test module must instantiate: {other:?}"),
        }
    }

    #[tokio::test]
    async fn aot_load_only_build_reports_no_jit_compilation() {
        assert!(
            !crate::jit_supported(),
            "an aot-load-only build cannot compile, so jit_supported must be false"
        );
    }

    #[tokio::test]
    async fn aot_load_only_build_interprets_instead_of_compiling() {
        let mut runtime_config = RuntimeConfig::default();
        runtime_config.jit.enabled = true;
        let store = Store::new_with_runtime_config(runtime_config);
        let instance = instantiate_wat(
            &store,
            r#"
            (module
              (func (export "run") (result i32)
                i32.const 42))
            "#,
        )
        .await;

        let result = run_module_function(&instance, &store, "run", &ResultValue::new(vec![])).await;
        match result {
            VMResult::Success(values) => {
                assert_eq!(values, ResultValue::new(vec![WasmValue::I32(42)]));
            }
            other => panic!(
                "the interpreter must produce the result when no native code is installed: {other:?}"
            ),
        }
        let stats = store.jit_cache_stats();
        assert_eq!(
            stats.compiled_functions, 0,
            "an aot-load-only build must not compile at runtime"
        );
    }
}
