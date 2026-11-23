//mod core;
#[macro_use]
mod trace;
/// Core-module instantiation and host-function linking entry points.
#[doc(hidden)]
pub(crate) mod instantiate;
/// Optional lazy baseline JIT support and cache statistics.
pub(crate) mod jit;
pub(crate) mod memory_effect;
#[cfg(test)]
mod reentrancy_guard;
pub(crate) mod scheduler;
pub(crate) mod trap_context;
#[allow(clippy::missing_safety_doc)]
pub(crate) mod vm;
/// Values returned by guest functions.
pub use crate::common::ResultValue;
/// Creates a host alias for an exported guest function.
pub use instantiate::aliasing;
/// Instantiates a parsed core WebAssembly module in a store.
pub use instantiate::instantiate;
/// Returns whether the optional JIT is available in this build and on this target.
///
/// Only the `jit` feature can compile WebAssembly at runtime, so this is
/// `false` in an `aot-load`-only build even though such a build can execute
/// precompiled code installed by an AoT loader.
pub fn jit_supported() -> bool {
    cfg!(feature = "jit") && jit::supported()
}
/// A completion delivered by an [`ExecutionDriver`].
pub use memory_effect::Completion;
/// Data carried when a pending operation completes.
pub use memory_effect::CompletionPayload;
/// An opaque pending asynchronous host call a driver polls to completion.
pub use memory_effect::HostCallFuture;
/// The opaque result of a completed asynchronous host call.
pub use memory_effect::HostCallOutcome;
/// A pending future returned by an asynchronous host callback.
pub use memory_effect::HostCallPending;
#[cfg(feature = "threads")]
/// A pending wait on shared WebAssembly memory.
pub use memory_effect::MemoryWaitPending;
/// Work the runtime asks an [`ExecutionDriver`] to schedule.
pub use memory_effect::PendingOp;
#[cfg(feature = "unstable-internals")]
/// A reserved pending operation for future guest async support.
pub use memory_effect::WasmAsyncPending;
/// Trait for integrating runtime pending work with an embedder executor.
pub use scheduler::{ExecutionDriver, TokioDriver};
/// Reads an exported global from an instance.
pub use vm::get_global;
/// Calls a named exported function with the default Tokio-backed driver.
pub use vm::run_module_function;
/// Calls a named exported function with a caller-supplied async driver.
pub use vm::run_module_function_with_driver;
