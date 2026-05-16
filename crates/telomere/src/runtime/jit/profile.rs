use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        OnceLock,
    },
    time::Instant,
};

macro_rules! jit_profile_counters {
    ($($name:ident => $label:literal),+ $(,)?) => {
        #[allow(dead_code)]
        #[derive(Clone, Copy)]
        #[repr(usize)]
        pub(crate) enum Counter {
            $($name,)+
            Count,
        }

        const COUNTER_LABELS: [&str; Counter::Count as usize] = [
            $($label,)+
        ];

        static COUNTERS: [AtomicU64; Counter::Count as usize] =
            [const { AtomicU64::new(0) }; Counter::Count as usize];
    };
}

jit_profile_counters! {
    CompileAttempt => "compile.attempt",
    CompileHit => "compile.hit",
    CompileAccepted => "compile.accepted",
    CompileRejectMaxZero => "compile.reject.max_zero",
    CompileRejectDisabled => "compile.reject.disabled",
    CompileRejectKnownRejected => "compile.reject.known_rejected",
    CompileRejectEmit => "compile.reject.emit",
    CompileRejectAllocationLen => "compile.reject.allocation_len",
    CompileRejectTooLarge => "compile.reject.too_large",
    CompileRejectDisabledAfterEmit => "compile.reject.disabled_after_emit",
    CompileRejectFromBytes => "compile.reject.from_bytes",
    CompileRejectRuntimeStub => "compile.reject.runtime_stub",
    CompileRejectRuntimeContinuationStub => "compile.reject.runtime_continuation_stub",
    CompileBytes => "compile.bytes",
    EmitInlineI32Load => "emit.inline_i32_load",
    EmitInlineI32Store => "emit.inline_i32_store",
    EmitGlobalGetInline => "emit.global_get.inline",
    EmitGlobalSetInline => "emit.global_set.inline",
    EmitMemoryFill => "emit.memory_fill",
    EmitMemoryCopy => "emit.memory_copy",
    EmitStackFlush => "emit.stack_flush",
    EmitStackFlushSlot => "emit.stack_flush.slot",
    EmitStackReload => "emit.stack_reload",
    EmitStackReloadSlot => "emit.stack_reload.slot",
    EmitHelperCall => "emit.helper_call",
    EmitDirectCallFast => "emit.direct_call.fast",
    EmitDirectCallHelper => "emit.direct_call.helper",
    EmitIndirectCallHelper => "emit.indirect_call.helper",
    RuntimePushI32 => "runtime.push_i32",
    RuntimePopI32 => "runtime.pop_i32",
    RuntimeDirectCallHelper => "runtime.direct_call.helper",
    RuntimeDirectCallFast => "runtime.direct_call.fast",
    RuntimeRejectedWasmCallFast => "runtime.rejected_wasm_call.fast",
    RuntimeIndirectCallHelper => "runtime.indirect_call.helper",
    RuntimeMemoryFill => "runtime.memory_fill",
    RuntimeMemoryCopy => "runtime.memory_copy",
    RuntimeMemorySize => "runtime.memory_size",
    RuntimeMemoryGrow => "runtime.memory_grow",
    RuntimeGlobalGet => "runtime.global_get",
    RuntimeGlobalSet => "runtime.global_set",
    RuntimeNumericHelper => "runtime.numeric_helper",
    RuntimeFloatHelper => "runtime.float_helper",
    ExitFallbackIndex => "exit.fallback_index",
    ExitFallbackPtr => "exit.fallback_ptr",
    ExitContinuePtr => "exit.continue_ptr",
    ExitTrap => "exit.trap",
    ExitDone => "exit.done",
    ExitPending => "exit.pending",
    ExitKeepGoing => "exit.keep_going",
    ExitUnknown => "exit.unknown",
}

static STARTED_AT: OnceLock<Instant> = OnceLock::new();
static REPORTED: AtomicBool = AtomicBool::new(false);

thread_local! {
    static REPORT_GUARD: ReportGuard = const { ReportGuard };
}

pub(crate) struct RunGuard {
    _private: (),
}

impl RunGuard {
    pub(crate) fn new() -> Self {
        if enabled() {
            init();
        }
        Self { _private: () }
    }
}

struct ReportGuard;

impl Drop for ReportGuard {
    fn drop(&mut self) {
        if enabled()
            && REPORTED
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        {
            report();
        }
    }
}

pub(crate) fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var("TELOMERE_JIT_PROFILE")
            .map(|value| value != "0" && !value.eq_ignore_ascii_case("false"))
            .unwrap_or(false)
    })
}

pub(crate) fn count(counter: Counter) {
    if enabled() {
        init();
        COUNTERS[counter as usize].fetch_add(1, Ordering::Relaxed);
    }
}

pub(crate) fn add(counter: Counter, value: u64) {
    if enabled() && value != 0 {
        init();
        COUNTERS[counter as usize].fetch_add(value, Ordering::Relaxed);
    }
}

pub(crate) fn count_exit(kind: u64) {
    let counter = match kind {
        super::abi::JitNativeExit::FALLBACK_INDEX => Counter::ExitFallbackIndex,
        super::abi::JitNativeExit::FALLBACK_PTR => Counter::ExitFallbackPtr,
        super::abi::JitNativeExit::CONTINUE_PTR => Counter::ExitContinuePtr,
        super::abi::JitNativeExit::TRAP => Counter::ExitTrap,
        super::abi::JitNativeExit::DONE => Counter::ExitDone,
        super::abi::JitNativeExit::PENDING => Counter::ExitPending,
        super::abi::JitNativeExit::KEEP_GOING => Counter::ExitKeepGoing,
        _ => Counter::ExitUnknown,
    };
    count(counter);
}

fn init() {
    STARTED_AT.get_or_init(Instant::now);
    REPORT_GUARD.with(|_| {});
}

fn report() {
    let elapsed = STARTED_AT.get().map(Instant::elapsed);
    let top_n = std::env::var("TELOMERE_JIT_PROFILE_TOP")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(16);
    let mut entries = COUNTER_LABELS
        .iter()
        .enumerate()
        .filter_map(|(index, label)| {
            let count = COUNTERS[index].load(Ordering::Relaxed);
            (count != 0).then_some((*label, count))
        })
        .collect::<Vec<_>>();
    entries.sort_unstable_by(|lhs, rhs| rhs.1.cmp(&lhs.1).then_with(|| lhs.0.cmp(rhs.0)));

    if let Some(elapsed) = elapsed {
        eprintln!(
            "[telomere-jit-profile] elapsed_ms={:.3} nonzero_counters={}",
            elapsed.as_secs_f64() * 1000.0,
            entries.len()
        );
    } else {
        eprintln!(
            "[telomere-jit-profile] elapsed_ms=unknown nonzero_counters={}",
            entries.len()
        );
    }
    for (label, count) in entries.into_iter().take(top_n) {
        eprintln!("[telomere-jit-profile] {label}={count}");
    }
}
