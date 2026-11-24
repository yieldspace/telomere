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
#[cfg(feature = "aot-load")]
pub(crate) mod aot_artifact;
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
use std::sync::atomic::{AtomicUsize, Ordering};

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
        VMResult::ResourceLimitExceeded => return VMResult::ResourceLimitExceeded,
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

/// #191: direct native-to-native entry.
///
/// The caller (a JIT call helper in `vm::call`) has already prepared
/// the callee frame through the ordinary call path — so the
/// value-stack reservation that bounds recursion depth is charged —
/// and resolved the callee's compiled function from the per-store
/// entry table (the `StoreInner` slot for the callee's funcaddr,
/// upgraded from its `Weak`). This function is the callee's entry
/// boundary: it invokes the native entry point and resolves a trap
/// exit's site id against the callee's own trap-site table, exactly
/// like [`enter_current_frame_raw`] does for interpreter-side entries.
///
/// It deliberately does not consult the store cache lock: the caller
/// holds the `Arc<CompiledFunction>` across the call, which keeps the
/// executable code alive even if the cache evicts the function
/// mid-flight (eviction only drops the cache's own `Arc`). Recency is
/// refreshed separately through
/// [`StoreJitCache::note_direct_call`].
///
/// # Safety
/// - `ctx.current_frame` must be the prepared frame of the function
///   `compiled` was built from, with `ctx.local_base_ptr` pointing at
///   that frame's local area (the state `prepare_jit_wasm_call`
///   leaves behind).
/// - `compiled` must be the active compiled function for that frame's
///   funcaddr in this store.
#[cfg(feature = "aot-load")]
pub(crate) unsafe fn enter_compiled_from_jit_call(
    ctx: &mut ExecuteContext<'_>,
    compiled: &CompiledFunction,
) -> JitNativeExit {
    let code_base = ctx.code();
    // PR #293 (Codex P1 on #191): the same host-stack floor check as
    // the generic entry -- decline to the interpreter fallback exit
    // when the remaining host stack runs low, instead of nesting
    // another native frame the stack may not hold.
    if !native_entry_stack_available(ctx) {
        profile::count(profile::Counter::RuntimeEntryStackFallback);
        return JitNativeExit::fallback_pc(code_base);
    }
    let exit = unsafe {
        (compiled.entry())(
            ctx as *mut ExecuteContext<'_>,
            code_base,
            ctx.local_base_ptr,
        )
    };
    let exit = resolve_trap_site(exit, compiled, code_base, ctx);
    unsafe { absolutize_fallback_index(exit, code_base) }
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
        VMResult::ResourceLimitExceeded => return VMResult::ResourceLimitExceeded,
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
    let mut guard = JitInterpreterStopGuard::new(ctx, stop_pc);
    // Dispatch through the guard's retained borrow (see the guard's
    // definition for the provenance argument).
    let ctx = guard.context();
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
pub(crate) fn should_stop_interpreter_at(ctx: &ExecuteContext<'_>, pc: *const Instr) -> bool {
    // #190: the stop target lives on the execution context, not in a
    // thread local, so this per-dispatch check is a single compare
    // against a field of the already-live context pointer.
    //
    // The null check from the thread-local form is folded into the
    // comparison: `pc` is a decoded-instruction pointer, non-null by
    // construction at every call site, so an inactive (null) stop target
    // can never equal it. Keeping this a single equality matters -- the
    // two-condition form made LLVM materialise the boolean through a
    // `setcc`/`and` chain in several handlers instead of one fused
    // compare-and-branch, which measured slower than the TLS read it
    // replaced.
    ctx.jit_stop_pc == pc
}

#[cfg(feature = "aot-load")]
pub(crate) fn interpreter_stop_active(ctx: &ExecuteContext<'_>) -> bool {
    // Return-call and function-return handlers must leave the interpreter loop
    // whenever any bridge owns control. The loop then decides whether the
    // observed continuation is the requested stop PC or another VM PC to resume.
    !ctx.jit_stop_pc.is_null()
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

/// Host-stack headroom (bytes) kept in reserve below the deepest
/// native entry. Between one entry check and the next, a level spends
/// the callee's native frame plus the call-helper crossing (measured
/// at roughly 0.5 KiB for the minimal self-call frame on x86_64
/// Linux), and a slow-path entry may additionally run the compiler
/// inside the entry; the reserve covers both with a wide margin while
/// staying small next to the smallest host stack worth protecting
/// (1 MiB).
#[cfg(feature = "aot-load")]
const NATIVE_ENTRY_STACK_RESERVE: usize = 256 * 1024;

/// Host-stack budget (bytes) used when the OS cannot report the
/// current thread's stack bounds: native entries may then spend at
/// most this much stack below the first entry of the context. Every
/// supported JIT target reports real bounds, so this is a
/// conservative fallback, not the expected path.
#[cfg(feature = "aot-load")]
const NATIVE_ENTRY_STACK_FALLBACK_BUDGET: usize = 512 * 1024;

/// An address inside the current host stack frame. The helper is
/// `#[inline(always)]` and the check below is too: the entry
/// boundaries run once per native call, so the steady-state check
/// must be a field load and a compare, not a call. After inlining,
/// the marker lives in the entry boundary's own frame -- one frame
/// below the point the callee is entered from, which is on the
/// conservative (lower) side for a downward-growing stack and well
/// inside the multi-KiB reserve.
#[cfg(feature = "aot-load")]
#[inline(always)]
fn current_stack_pointer() -> usize {
    // `MaybeUninit` so the marker costs no initialising store; only
    // its address is observed, never its contents.
    let marker = std::mem::MaybeUninit::<u8>::uninit();
    marker.as_ptr() as usize
}

/// The lowest address of the current thread's stack, as reported by
/// the OS, or `None` when the platform cannot report it. All
/// supported JIT targets have downward-growing stacks.
#[cfg(all(feature = "aot-load", target_os = "linux", target_env = "gnu"))]
fn thread_stack_low_bound() -> Option<usize> {
    unsafe {
        let mut attr: libc::pthread_attr_t = std::mem::zeroed();
        if libc::pthread_getattr_np(libc::pthread_self(), &mut attr) != 0 {
            return None;
        }
        let mut addr: *mut libc::c_void = std::ptr::null_mut();
        let mut size: libc::size_t = 0;
        let rc = libc::pthread_attr_getstack(&attr, &mut addr, &mut size);
        libc::pthread_attr_destroy(&mut attr);
        if rc != 0 || addr.is_null() || size == 0 {
            None
        } else {
            // `pthread_attr_getstack` reports the lowest address of
            // the stack region for a downward-growing stack.
            Some(addr as usize)
        }
    }
}

#[cfg(all(feature = "aot-load", target_os = "macos"))]
fn thread_stack_low_bound() -> Option<usize> {
    unsafe {
        let thread = libc::pthread_self();
        let top = libc::pthread_get_stackaddr_np(thread);
        let size = libc::pthread_get_stacksize_np(thread);
        if top.is_null() || size == 0 {
            None
        } else {
            // `pthread_get_stackaddr_np` returns the highest address
            // of the downward-growing stack.
            Some((top as usize).saturating_sub(size))
        }
    }
}

#[cfg(all(
    feature = "aot-load",
    not(any(all(target_os = "linux", target_env = "gnu"), target_os = "macos"))
))]
fn thread_stack_low_bound() -> Option<usize> {
    None
}

/// Whether the host stack has room for another native entry (PR #293,
/// Codex P1 on #191).
///
/// The first native entry of a context establishes the floor on the
/// context: the OS-reported low bound of the current thread's stack
/// plus [`NATIVE_ENTRY_STACK_RESERVE`], or a fixed budget below the
/// first entry when the OS cannot report bounds. Later entries only
/// compare the current stack pointer against that floor. Anchoring
/// the bound to the thread's real stack -- rather than counting
/// entries on the context -- keeps it correct when execution
/// re-enters through a host function under a fresh context on the
/// same thread: the stack pointer still reflects every frame the
/// outer execution holds, where a context-carried counter would
/// silently reset. When this returns false the entry boundaries
/// decline the native entry and the frame runs in the interpreter
/// instead, whose tail-call dispatch spends no host stack per level,
/// so recursion depth stays bounded by the value-stack accounting
/// alone (#166) and reaches the same `StackOverflow` the interpreter
/// produces.
#[cfg(feature = "aot-load")]
#[inline(always)]
fn native_entry_stack_available(ctx: &mut ExecuteContext<'_>) -> bool {
    if ctx.jit_native_stack_floor == 0 {
        ctx.jit_native_stack_floor = match thread_stack_low_bound() {
            Some(low) => low.saturating_add(NATIVE_ENTRY_STACK_RESERVE),
            None => current_stack_pointer().saturating_sub(NATIVE_ENTRY_STACK_FALLBACK_BUDGET),
        };
    }
    current_stack_pointer() >= ctx.jit_native_stack_floor
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
    // #191: there is deliberately no separate native-entry depth
    // *counter* here (the pre-#191 TLS counter stopped at 256 nested
    // entries). Every native entry — from the interpreter, from a lazy
    // call, or from a direct native-to-native call — runs on a frame
    // that was prepared through `function_call_cached`, whose
    // reservation (locals + call record + the callee's validated
    // maximum operand depth, #186) is charged against the store's
    // value-stack byte limit (#126 `limits.value_stack_bytes`) and
    // fails with `StackOverflow` before the frame exists. Native
    // nesting depth is therefore bounded by the same accounting that
    // bounds interpreter recursion (#166).
    //
    // PR #293 (Codex P1 on #191): that accounting alone is not a
    // host-stack bound. Each level also spends real host stack — a
    // native frame plus the call-helper crossing, measured at roughly
    // 0.5 KiB for the minimal self-call frame — which the reservation
    // does not charge: at the default limits the minimal frame nests
    // ~3.2k entries (~1.5 MiB of host stack), so on a 1 MiB host
    // thread the process aborted before the value-stack bound was
    // reached. The host-stack floor check therefore also gates this
    // entry: when the remaining stack runs low, the entry declines
    // with `Unimplemented` and the frame runs in the interpreter
    // (whose dispatch is flat), so deep recursion reaches the same
    // value-stack `StackOverflow` on any host stack size instead of
    // terminating the embedding process. The
    // `jit_deep_recursion_traps_as_stack_overflow` test pins the
    // roomy-stack behaviour; `jit_recursion_on_small_stacks` pins the
    // 1 MiB case.
    if !native_entry_stack_available(ctx) {
        profile::count(profile::Counter::RuntimeEntryStackFallback);
        return VMResult::Unimplemented;
    }
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
#[cfg(feature = "aot-load")]
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
    let max_bytes = store.runtime_config().limits.max_code_cache_bytes;
    // #171: metering is fixed at store creation and this cache is
    // private to the store, so whether the store is metered can be
    // baked into the emitted code.
    let metered = store.metering_ref().is_some();
    store
        .jit_cache()
        .get_or_compile(funcaddr, body, runtime, max_bytes, metered)
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
struct JitInterpreterStopGuard<'g, 'a> {
    // The guard retains the bridge entry point's own mutable borrow of
    // the context for its whole lifetime. Bridge dispatch reborrows from
    // this borrow (see `context`), and the saved value is restored
    // through it on drop, so every access to `jit_stop_pc` in a bridge
    // flows through one borrow chain. Retaining a raw pointer to the
    // field derived from a shorter reborrow instead would lose that
    // pointer's provenance as soon as the bridge reborrowed the context
    // for dispatch, and the restore on drop would write through an
    // invalidated pointer.
    ctx: &'g mut ExecuteContext<'a>,
    previous: *const Instr,
}

#[cfg(feature = "aot-load")]
impl<'g, 'a> JitInterpreterStopGuard<'g, 'a> {
    fn new(ctx: &'g mut ExecuteContext<'a>, stop_pc: *const Instr) -> Self {
        // Continuation bridges can re-enter the interpreter while an outer bridge
        // is already active. The innermost native caller owns the active stop PC;
        // the previous target is restored when that bridge returns.
        let previous = ctx.jit_stop_pc;
        ctx.jit_stop_pc = stop_pc;
        Self { ctx, previous }
    }

    /// The context, reborrowed from the borrow the guard retains.
    /// Bridge dispatch must go through this accessor so that its reads
    /// of `jit_stop_pc` share the guard's borrow provenance.
    fn context(&mut self) -> &mut ExecuteContext<'a> {
        &mut *self.ctx
    }
}

#[cfg(feature = "aot-load")]
impl Drop for JitInterpreterStopGuard<'_, '_> {
    fn drop(&mut self) {
        // Restored through the borrow the guard retained since `new`;
        // every reborrow handed out by `context` has ended by the time
        // the bridge entry point drops the guard.
        self.ctx.jit_stop_pc = self.previous;
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
    let env_traced = std::env::var_os("TELOMERE_JIT_TRACE_FALLBACK").is_some();
    let event_traced = tracing::level_enabled!(tracing::Level::DEBUG);
    if !env_traced && !event_traced {
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
    if event_traced {
        tracing::debug!(
            target: crate::diagnostics::TARGET_JIT,
            kind,
            funcidx = funcidx.as_str(),
            pc = pc_index,
            "jit execution fell back to the interpreter"
        );
    }
    if !env_traced {
        return;
    }
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
