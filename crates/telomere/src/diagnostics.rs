//! Embedder observability: the [`tracing`] events Telomere emits.
//!
//! Telomere reports what a guest did through [`tracing`] events on the
//! documented targets in this module. An embedder subscribes with any
//! [`tracing`] subscriber (for example `tracing-subscriber`) and routes the
//! events to its own logging or metrics pipeline. The events are compiled in
//! unconditionally: no Cargo feature is required, in debug or release
//! profiles alike.
//!
//! Until this contract existed, the runtime enabled `tracing`'s
//! `release_max_level_off` feature, which — through Cargo feature
//! unification — statically disabled release-level tracing for every crate
//! in a downstream dependency graph, including the embedder's own
//! application. That feature is gone; whether release events are recorded
//! is now the embedder's choice of subscriber and filter, and the runtime
//! cost of that choice is measured in the
//! [diagnostics guide](https://github.com/yieldspace/telomere/blob/main/docs/core/diagnostics.md).
//!
//! # Targets
//!
//! | Target | Level | When |
//! | --- | --- | --- |
//! | [`TARGET_TRAP`] | `WARN` | A guest trap is recorded for [`Store::take_last_trap`]. |
//! | [`TARGET_JIT`] | `DEBUG` | A JIT compilation is rejected, or native execution falls back to the interpreter. |
//! | [`TARGET_JIT`] | `INFO` | One line of the `TELOMERE_JIT_PROFILE` report, while that facility is enabled. |
//! | [`TARGET_STACK`] | `DEBUG` | The interpreter stack-overflow trace fires. |
//!
//! The trap event carries structured fields: `kind` (the stable
//! [`TrapKind`](crate::TrapKind) label), `total_frames`, `truncated`, and
//! the innermost frame's `funcidx` and `pc_index` when they could be
//! resolved. The full owned record — frames and resolved names — remains
//! available synchronously through [`Store::take_last_trap`]; the event is
//! the push notification, not a replacement for it.
//!
//! Telomere also emits unstructured `trace` events under the emitting
//! module's path (instantiation and section-parser detail). Those are
//! developer traces, not a stable contract; the targets above are the
//! contract. The instruction parser's per-opcode traces are
//! debug-build-only: its functions recurse once per control-nesting level
//! inside the documented 512 KiB release stack budget, and live
//! callsites measurably grow those frames (see `docs/core/diagnostics.md`
//! for the measurements).
//!
//! # Environment variables
//!
//! The historical environment-variable switches remain available as
//! command-line conveniences: `TELOMERE_JIT_TRACE_COMPILE`,
//! `TELOMERE_JIT_TRACE_FALLBACK`, and `TELOMERE_STACK_TRACE_OVERFLOW` still
//! print the same diagnostics to stderr, and the same information is emitted
//! as events on the targets above whether or not the variables are set.
//! The profiling facilities (`TELOMERE_VM_PROFILE` under the `vm-profile`
//! feature, `TELOMERE_JIT_PROFILE`, and the `vm-diagnostics` dumps and
//! instruction budget) stay environment-driven developer tools; the
//! [diagnostics guide](https://github.com/yieldspace/telomere/blob/main/docs/core/diagnostics.md)
//! lists every variable and what it gates.
//!
//! [`Store::take_last_trap`]: crate::Store::take_last_trap

use crate::runtime::trap_context::TrapContext;

/// Target of the event emitted when a guest trap is recorded.
///
/// Emitted at `WARN` level each time an outermost guest call (or a trapping
/// `start` function) leaves a trap record for
/// [`Store::take_last_trap`](crate::Store::take_last_trap). Fields: `kind`,
/// `total_frames`, `truncated`, `funcidx`, `pc_index`.
pub const TARGET_TRAP: &str = "telomere::trap";

/// Target of the JIT diagnostic events.
///
/// Emitted at `DEBUG` level when a function is rejected during JIT
/// compilation (fields: `reason`, or `pc` / `op` for an emission rejection)
/// and when native execution falls back to the interpreter (fields: `kind`,
/// `funcidx`, `pc`). While the `TELOMERE_JIT_PROFILE` facility is enabled,
/// its report lines are also emitted on this target at `INFO` level.
pub const TARGET_JIT: &str = "telomere::jit";

/// Target of the interpreter stack-overflow trace events.
///
/// Emitted at `DEBUG` level with the same text the
/// `TELOMERE_STACK_TRACE_OVERFLOW` stderr trace prints.
pub const TARGET_STACK: &str = "telomere::stack";

/// Emits the [`TARGET_TRAP`] event for a captured trap context.
///
/// Called from the single trap-recording choke point, so every outermost
/// boundary (sync, async-driver, and instantiate-with-start) reports
/// identically. Only the cheap scalar context is recorded as fields; name
/// resolution stays in [`TrapInfo`](crate::TrapInfo) construction, which
/// the embedder triggers by taking the trap.
#[cold]
pub(crate) fn emit_trap_event(context: &TrapContext) {
    let Some(kind) = crate::TrapKind::from_vm_result(&context.result) else {
        return;
    };
    let frame = context.frames.first();
    tracing::warn!(
        target: TARGET_TRAP,
        kind = kind.as_str(),
        total_frames = context.total_frames,
        truncated = context.truncated,
        funcidx = ?frame.and_then(|frame| frame.funcidx),
        pc_index = ?frame.and_then(|frame| frame.pc_index),
        "guest trap recorded"
    );
}
