//mod core;
#[macro_use]
mod trace;
/// Core-module instantiation and host-function linking entry points.
#[warn(missing_docs)]
pub(crate) mod instantiate;
/// Optional lazy baseline JIT support and cache statistics.
pub mod jit;
pub(crate) mod memory_effect;
pub(crate) mod scheduler;
#[allow(clippy::missing_safety_doc)]
pub(crate) mod vm;
/// Values returned by guest functions.
pub use crate::common::ResultValue;
/// Creates a host alias for an exported guest function.
pub use instantiate::aliasing;
/// Instantiates a parsed core WebAssembly module in a store.
pub use instantiate::instantiate;
/// Instantiates a native module containing asynchronous host callbacks.
pub use instantiate::instantiate_native_async_module;
/// Instantiates a native module containing synchronous host callbacks.
pub use instantiate::instantiate_native_module;
/// Replaces an exported guest function with an asynchronous host callback.
pub use instantiate::link_async_host_function_with_export_name;
/// Replaces a guest function by index with an asynchronous host callback.
pub use instantiate::link_async_host_function_with_function_idx;
/// Replaces an exported guest function with a synchronous host callback.
pub use instantiate::link_host_function_with_export_name;
/// Replaces a guest function by index with a synchronous host callback.
pub use instantiate::link_host_function_with_function_idx;
/// Returns whether the optional JIT is available in this build and on this target.
pub use jit::supported as jit_supported;
/// A completion delivered by an [`ExecutionDriver`].
pub use memory_effect::Completion;
/// Data carried when a pending operation completes.
pub use memory_effect::CompletionPayload;
/// A pending future returned by an asynchronous host callback.
pub use memory_effect::HostCallPending;
#[cfg(feature = "threads")]
/// A pending wait on shared WebAssembly memory.
pub use memory_effect::MemoryWaitPending;
/// Work the runtime asks an [`ExecutionDriver`] to schedule.
pub use memory_effect::PendingOp;
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
/// Completes the current host-call frame through the runtime's special path.
pub use vm::special_function_return;
