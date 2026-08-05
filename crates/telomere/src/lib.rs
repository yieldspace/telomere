#![warn(missing_docs)]
#![warn(unnameable_types)]

//! Core WebAssembly engine for telomere.
//!
//! This crate owns everything below the Component Model: the binary parser, the
//! validator-facing optimizer that lowers a parsed module into the runtime's
//! instruction representation, the interpreter and its scheduler, host function
//! linking through [`Registry`], and the optional function-local lazy baseline
//! JIT behind the `jit` feature. It is written from scratch and does not depend
//! on Cranelift or on the `wasmparser`/`wasmtime` crates. Embedders normally
//! start from [`Store`], [`instantiate`], and [`run_module_function`].
//!
//! Without the `threads` feature, shared memories and `0xFE` atomic opcodes are
//! rejected during parsing and validation, and `tokio` is absent from this
//! crate's normal dependency graph.
//!
//! # Core embedding
//!
//! Parse a module, instantiate it in a [`Store`], then call one of its exports.
//! The example uses a single-thread Tokio runtime because the core APIs are
//! asynchronous even when the guest has no asynchronous imports.
//!
//! ```
//! use telomere::{
//!     instantiate, IoReadBinaryReader, Registry, ResultValue, Store, VMResult, WasmParser,
//!     WasmValue,
//! };
//!
//! let bytes = wat::parse_str(
//!     r#"(module
//!         (func (export "add") (param i32 i32) (result i32)
//!             local.get 0
//!             local.get 1
//!             i32.add)
//!         (func (export "trap") unreachable))"#,
//! )
//! .expect("the inline module is valid WebAssembly");
//! let mut reader = IoReadBinaryReader::from(&bytes[..]);
//! let module = WasmParser::new(&mut reader)
//!     .parse_module()
//!     .expect("the module parses");
//! let store = Store::new();
//! let registry = Registry::new();
//! let runtime = tokio::runtime::Builder::new_current_thread()
//!     .build()
//!     .expect("Tokio runtime builds");
//!
//! let (instance, result) = runtime.block_on(async {
//!     let instance = match instantiate(module, &store, &registry).await {
//!         VMResult::Success(instance) => instance,
//!         failure => panic!("instantiation failed: {failure:?}"),
//!     };
//!     let result = telomere::run_module_function(
//!         &instance,
//!         &store,
//!         "add",
//!         &ResultValue::new(vec![WasmValue::I32(20), WasmValue::I32(22)]),
//!     )
//!     .await;
//!     (instance, result)
//! });
//! match result {
//!     VMResult::Success(values) => {
//!         assert_eq!(values, ResultValue::new(vec![WasmValue::I32(42)]));
//!     }
//!     failure => panic!("guest call failed: {failure:?}"),
//! }
//! let trapped = runtime.block_on(telomere::run_module_function(
//!     &instance,
//!     &store,
//!     "trap",
//!     &ResultValue::new(vec![]),
//! ));
//! assert!(matches!(trapped, VMResult::Unreachable));
//! let trap = store.take_last_trap().expect("the trap was captured");
//! assert_eq!(trap.kind, telomere::TrapKind::Unreachable);
//! ```
//!
//! For a complete standalone crate with feature-ladder commands, see the
//! [minimal embedder](https://github.com/yieldspace/telomere/tree/main/examples/minimal-embedder).

#[doc(hidden)]
pub(crate) mod binary;
#[macro_use]
#[doc(hidden)]
pub(crate) mod common;
/// Component-model adapters form the documented boundary used by component crates.
#[warn(missing_docs)]
pub mod component_support;
#[doc(hidden)]
pub(crate) mod parser;
#[doc(hidden)]
pub(crate) mod runtime;

/// Raw host-linking ABI kept public for the default embedding capability.
///
/// This compatibility carve-out exposes interpreter representation required by
/// synchronous and asynchronous host-function linking. It remains public so the
/// documented support matrix and embedding examples work in default builds; its
/// eventual closure is tracked by issue #216.
pub mod host_abi;

/// Unstable interpreter internals for opt-in downstream integrations.
#[cfg(feature = "unstable-internals")]
#[doc(hidden)]
pub mod unstable_internals;

/// Resolves the measurement-only optimizer pipeline switch when the
/// `measure-switches` feature is enabled.
#[cfg(feature = "measure-switches")]
#[path = "parser/core/optimizer/measure_switches.rs"]
pub mod measure_switches;

/// Maximum nesting depth for `block`, `loop`, and `if` instructions accepted by [`WasmParser`].
///
/// This limit implements the optimized-build 512 KiB input-parser stack-budget policy. The root
/// is at `depth = 0`, and parsing rejects `depth > limit`, so 512 nested constructs are accepted
/// and 513 are rejected with [`WasmParserError::NestingTooDeep`].
///
/// See the [parser limits guide](https://github.com/yieldspace/telomere/blob/main/docs/core/parser-limits.md)
/// for the policy scope and measurement boundary.
pub const MAX_CONTROL_NESTING_DEPTH: u32 = 512;

/// Reads core WebAssembly bytes from any value that implements [`std::io::Read`].
pub use binary::IoReadBinaryReader;
/// Configures diagnostic metadata retained by a [`Store`].
pub use common::DiagnosticsConfig;
/// A cloneable, store-bound handle to an instantiated WebAssembly module.
pub use common::InstanceHandle;
/// Explains why metered guest execution stopped.
pub use common::InterruptReason;
/// Configures the optional lazy baseline JIT for a [`Store`].
pub use common::JitConfig;
/// Sets resource limits for memories created by a [`Store`].
pub use common::MemoryConfig;
/// Configures Store-scoped guest execution metering.
pub use common::MeteringConfig;
/// Controls fuel and cancellation for an enabled Store's metered execution.
pub use common::MeteringHandle;
/// The parsed, validated representation supplied to [`instantiate`].
pub use common::Module;
/// Resolves imported module names to already-instantiated instances.
pub use common::Registry;
/// Ordered values passed to, or returned from, an exported WebAssembly function.
pub use common::ResultValue;
/// Groups resource and execution settings for a [`Store`].
pub use common::RuntimeConfig;
/// One captured frame in a guest trap report.
pub use common::TrapFrame;
/// The runtime category of a captured trap frame.
pub use common::TrapFrameKind;
/// Owned diagnostic data for a captured guest trap.
pub use common::TrapInfo;
/// A Telomere diagnostic label for a guest trap.
pub use common::TrapKind;
/// Guest execution success or a trap/linking failure.
pub use common::VMResult;
/// A value crossing the core WebAssembly host/guest boundary.
pub use common::WasmValue;
/// Owns runtime state, resources, and optional embedder state for instantiated modules.
pub use common::{Store, StoreState};
/// Identifies a WebAssembly proposal when parsing rejects an unsupported feature.
pub use parser::core::ProposalFeature;
/// Parses a core WebAssembly binary into a [`Module`].
pub use parser::core::WasmParser;
/// Reports malformed core WebAssembly input or an unsupported proposal.
pub use parser::core::WasmParserError;
/// Creates a host alias for an exported guest function.
pub use runtime::aliasing;
/// Reads an exported immutable or mutable global value from an instance.
pub use runtime::get_global;
/// Instantiates a parsed core WebAssembly module in a store.
pub use runtime::instantiate;
#[cfg(feature = "jit")]
/// Statistics for the optional store-local JIT cache.
///
/// This remains public because the minimal-embedder JIT configuration uses it
/// to verify that an enabled workload compiled.
pub use runtime::jit::JitCacheStats;
/// Reports whether this build and target can execute the optional JIT.
pub use runtime::jit_supported;

/// Instantiates a native module whose host functions return futures.
///
/// This exposes raw interpreter continuations to preserve default asynchronous
/// host linking advertised by Telomere's support matrix and embedding examples.
/// The compatibility carve-out is tracked by issue #216.
pub use runtime::instantiate::instantiate_native_async_module;
/// Replaces an exported guest function with an asynchronous host callback.
///
/// This raw callback ABI remains public for default asynchronous host linking;
/// issue #216 tracks its replacement.
pub use runtime::instantiate::link_async_host_function_with_export_name;
/// Replaces a guest function by index with an asynchronous host callback.
///
/// This raw callback ABI remains public for default asynchronous host linking;
/// issue #216 tracks its replacement.
pub use runtime::instantiate::link_async_host_function_with_function_idx;
/// Replaces an exported guest function with a synchronous host callback.
///
/// This raw callback ABI remains public for default synchronous host linking;
/// issue #216 tracks its replacement.
pub use runtime::instantiate::link_host_function_with_export_name;
/// Replaces a guest function by index with a synchronous host callback.
///
/// This raw callback ABI remains public for default synchronous host linking;
/// issue #216 tracks its replacement.
pub use runtime::instantiate::link_host_function_with_function_idx;
#[cfg(feature = "unstable-internals")]
/// Marks the current host call as returning through the runtime's special path.
///
/// This raw interpreter handler is available only for opt-in raw instruction
/// construction. Default builds expose no public way to construct the required
/// `Instr` sequence, so it is not part of the default host-linking carve-out.
/// Issue #216 tracks its replacement.
pub use runtime::vm::special_function_return;

/// Calls a named exported function using an embedder-provided async driver.
///
/// This exposes raw continuations so default asynchronous host linking can use
/// an embedder executor. Issue #216 tracks the compatibility carve-out.
pub use runtime::run_module_function_with_driver;
/// A completion delivered by an [`ExecutionDriver`].
///
/// This raw driver value remains public for default asynchronous host linking;
/// issue #216 tracks its replacement.
pub use runtime::Completion;
/// The result payload carried by a [`Completion`].
///
/// This raw driver value remains public for default asynchronous host linking;
/// issue #216 tracks its replacement.
pub use runtime::CompletionPayload;
/// Integrates pending WebAssembly operations with an embedder's async executor.
///
/// This raw driver trait remains public for default asynchronous host linking;
/// issue #216 tracks its replacement.
pub use runtime::ExecutionDriver;
/// A pending asynchronous host-function invocation.
///
/// This raw driver value remains public for default asynchronous host linking;
/// issue #216 tracks its replacement.
pub use runtime::HostCallPending;
#[cfg(feature = "threads")]
/// A pending wait on shared WebAssembly memory.
///
/// This raw driver value remains public for default asynchronous host linking
/// with threads; issue #216 tracks its replacement.
pub use runtime::MemoryWaitPending;
/// Work submitted by the runtime to an [`ExecutionDriver`].
///
/// This raw driver value remains public for default asynchronous host linking;
/// issue #216 tracks its replacement.
pub use runtime::PendingOp;
/// The default driver for async host calls and shared-memory waits.
///
/// This raw driver remains public for default asynchronous host linking; issue
/// #216 tracks its replacement.
pub use runtime::TokioDriver;
#[cfg(feature = "unstable-internals")]
/// A reserved pending operation for future guest async support.
///
/// No runtime producer currently emits this value, and [`TokioDriver`] rejects
/// it. It is therefore available only to opt-in integrations that construct raw
/// pending operations; issue #216 tracks its replacement.
pub use runtime::WasmAsyncPending;

/// Calls a named exported function using the default Tokio-backed driver.
pub use runtime::run_module_function;
