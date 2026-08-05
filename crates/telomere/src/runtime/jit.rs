#[cfg(feature = "jit")]
mod abi;
#[cfg(feature = "jit")]
mod backend;
#[cfg(feature = "jit")]
mod cache;
#[cfg(feature = "jit")]
pub(crate) mod profile;
#[cfg(feature = "jit")]
mod stubs;

#[cfg(feature = "jit")]
use crate::{
    common::{store::FunctionBody, ExecuteContext, Instr, VMResult},
    runtime::vm,
};
#[cfg(feature = "jit")]
use std::cell::Cell;
#[cfg(feature = "jit")]
use std::sync::atomic::{AtomicUsize, Ordering};

#[cfg(feature = "jit")]
thread_local! {
    static JIT_ENTRY_DEPTH: Cell<usize> = const { Cell::new(0) };
    static JIT_INTERPRETER_STOP_AT: Cell<*const Instr> = const { Cell::new(std::ptr::null()) };
}

#[cfg(feature = "jit")]
pub use abi::JitNativeExit;
#[cfg(feature = "jit")]
pub use cache::JitCacheStats;
#[cfg(feature = "jit")]
pub(crate) use cache::{CompiledFunction, StoreJitCache};

/// Returns whether the JIT is compiled in and available for the current target.
///
/// A `true` result only means the target can run JIT code; enable it for a
/// particular store with [`crate::JitConfig`].
pub fn supported() -> bool {
    jit_supported()
}

#[cfg(feature = "jit")]
fn jit_supported() -> bool {
    telomere_jit_codegen::target::supported()
}

#[cfg(not(feature = "jit"))]
fn jit_supported() -> bool {
    false
}

#[cfg(feature = "jit")]
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
        VMResult::UnalignedAtomic => return VMResult::UnalignedAtomic,
        VMResult::FuelExhausted => return VMResult::FuelExhausted,
        VMResult::Cancelled => return VMResult::Cancelled,
    };
    unsafe { handle_exit(exit, code_base, ctx) }
}

#[cfg(feature = "jit")]
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

#[cfg(feature = "jit")]
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
        VMResult::UnalignedAtomic => return VMResult::UnalignedAtomic,
        VMResult::FuelExhausted => return VMResult::FuelExhausted,
        VMResult::Cancelled => return VMResult::Cancelled,
    };
    if exit.kind == JitNativeExit::CONTINUE_PTR && exit.value == continuation as u64 {
        return unsafe { vm::call_code(continuation, ctx) };
    }
    unsafe { handle_exit(exit, code_base, ctx) }
}

#[cfg(feature = "jit")]
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
            other => {
                return JitNativeExit {
                    kind: JitNativeExit::TRAP,
                    value: abi::encode_trap_value(
                        abi::TRAP_SITE_INHERIT,
                        abi::vm_result_code(other),
                    ),
                };
            }
        }
    }
}

#[cfg(feature = "jit")]
pub(crate) fn should_stop_interpreter_at(pc: *const Instr) -> bool {
    JIT_INTERPRETER_STOP_AT.with(|stop| {
        let target = stop.get();
        !target.is_null() && target == pc
    })
}

#[cfg(feature = "jit")]
pub(crate) fn interpreter_stop_active() -> bool {
    // Return-call and function-return handlers must leave the interpreter loop
    // whenever any bridge owns control. The loop then decides whether the
    // observed continuation is the requested stop PC or another VM PC to resume.
    JIT_INTERPRETER_STOP_AT.with(|stop| !stop.get().is_null())
}

#[cfg(feature = "jit")]
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

#[cfg(feature = "jit")]
unsafe fn enter_current_frame_raw(ctx: &mut ExecuteContext<'_>) -> VMResult<JitNativeExit> {
    if !backend::supported() || !ctx.store.runtime_config().jit.enabled {
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
        ctx.cont = std::ptr::null();
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
                let max_bytes = ctx.store.runtime_config().jit.code_cache_max_bytes;
                let compiled = vm_try!(ctx
                    .store
                    .jit_cache()
                    .get_or_compile(funcaddr, &func.body, ctx.gc, max_bytes));
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

#[cfg(feature = "jit")]
fn resolve_trap_site(
    mut exit: JitNativeExit,
    compiled: &CompiledFunction,
    code_base: *const Instr,
    ctx: &mut ExecuteContext<'_>,
) -> JitNativeExit {
    if exit.kind != JitNativeExit::TRAP {
        return exit;
    }

    match abi::trap_site(exit.value) {
        abi::TRAP_SITE_UNKNOWN => {
            ctx.cont = std::ptr::null();
        }
        abi::TRAP_SITE_INHERIT => {}
        site => {
            ctx.cont = match compiled.trap_site_pc(site) {
                Some(pc) if !code_base.is_null() => unsafe { code_base.add(pc as usize) },
                Some(_) | None => std::ptr::null(),
            };
            exit.value = abi::encode_trap_value(abi::TRAP_SITE_INHERIT, abi::trap_code(exit.value));
        }
    }
    exit
}

#[cfg(feature = "jit")]
struct JitEntryDepthGuard;

#[cfg(feature = "jit")]
struct JitInterpreterStopGuard {
    previous: *const Instr,
}

#[cfg(feature = "jit")]
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

#[cfg(feature = "jit")]
impl Drop for JitEntryDepthGuard {
    fn drop(&mut self) {
        JIT_ENTRY_DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
    }
}

#[cfg(feature = "jit")]
impl Drop for JitInterpreterStopGuard {
    fn drop(&mut self) {
        JIT_INTERPRETER_STOP_AT.with(|stop| stop.set(self.previous));
    }
}

#[cfg(feature = "jit")]
unsafe fn handle_exit(
    exit: JitNativeExit,
    _code_base: *const crate::common::Instr,
    ctx: &mut ExecuteContext<'_>,
) -> VMResult<()> {
    match exit.kind {
        JitNativeExit::FALLBACK_INDEX => {
            profile::count_exit(exit.kind);
            ctx.cont = std::ptr::null();
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
            if abi::trap_site(exit.value) != abi::TRAP_SITE_INHERIT {
                ctx.cont = std::ptr::null();
            }
            abi::vm_result_from_code(abi::trap_code(exit.value))
        }
        JitNativeExit::KEEP_GOING => {
            profile::count_exit(exit.kind);
            ctx.cont = std::ptr::null();
            VMResult::InvalidOperand
        }
        _ => {
            profile::count_exit(exit.kind);
            ctx.cont = std::ptr::null();
            VMResult::InvalidOperand
        }
    }
}

#[cfg(feature = "jit")]
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
