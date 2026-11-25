//! Front-end phase counters (#199).
//!
//! Process-wide counters are useless for attributing work to one
//! load path when a process parses, loads, and executes many modules
//! concurrently, so these counters are **thread-local**: each thread
//! counts only the front-end work it performed itself. A caller that
//! snapshots the counters, runs one load path synchronously, and
//! snapshots again can therefore attribute the delta exactly — which
//! is what the AoT wast lane and the artifact integration tests use
//! to prove that loading an artifact skips the parse, optimize, and
//! emit phases instead of silently redoing them.
//!
//! The counters are deliberately always on: each increment is one
//! relaxed `Cell` update on a path that already does far more work
//! (parsing a module, optimizing a function, emitting native code).

use std::cell::Cell;

thread_local! {
    static PARSED_MODULES: Cell<u64> = const { Cell::new(0) };
    static OPTIMIZED_FUNCTIONS: Cell<u64> = const { Cell::new(0) };
    static DESERIALIZED_MODULES: Cell<u64> = const { Cell::new(0) };
    static EMITTED_FUNCTIONS: Cell<u64> = const { Cell::new(0) };
}

fn bump(counter: &'static std::thread::LocalKey<Cell<u64>>) {
    counter.with(|value| value.set(value.get().wrapping_add(1)));
}

/// A snapshot of the calling thread's front-end phase counters.
///
/// Compare two snapshots taken around a synchronous operation to see
/// which front-end phases that operation ran; see the module docs
/// for why the counters are thread-local.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrontendPhaseCounters {
    /// Core modules fully parsed from wasm bytes by
    /// [`crate::WasmParser::parse_module`] on this thread. Parsing
    /// includes lowering, so this is the parse+optimize front-end's
    /// entry count; [`Self::optimized_functions`] counts its
    /// per-function work.
    pub parsed_modules: u64,
    /// Functions run through the optimizer pipeline
    /// (`optimizer::optimize_function`) on this thread.
    pub optimized_functions: u64,
    /// Modules restored from a precompiled or AoT artifact on this
    /// thread — the artifact front-end that replaces parsing.
    pub deserialized_modules: u64,
    /// Functions emitted by the on-demand JIT compiler and inserted
    /// into a store's code cache on this thread. Functions installed
    /// from an AoT artifact do not pass through emission and never
    /// move this counter.
    pub emitted_functions: u64,
}

/// Returns the calling thread's current front-end phase counters.
pub fn frontend_phase_counters() -> FrontendPhaseCounters {
    FrontendPhaseCounters {
        parsed_modules: PARSED_MODULES.with(Cell::get),
        optimized_functions: OPTIMIZED_FUNCTIONS.with(Cell::get),
        deserialized_modules: DESERIALIZED_MODULES.with(Cell::get),
        emitted_functions: EMITTED_FUNCTIONS.with(Cell::get),
    }
}

pub(crate) fn note_module_parsed() {
    bump(&PARSED_MODULES);
}

pub(crate) fn note_function_optimized() {
    bump(&OPTIMIZED_FUNCTIONS);
}

pub(crate) fn note_module_deserialized() {
    bump(&DESERIALIZED_MODULES);
}

/// The only caller is the JIT cache, so this is unused in
/// non- builds.
#[allow(dead_code)]
pub(crate) fn note_function_emitted() {
    bump(&EMITTED_FUNCTIONS);
}
