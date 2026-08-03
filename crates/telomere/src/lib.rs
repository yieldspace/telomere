#![warn(missing_docs)]

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
//!             i32.add))"#,
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
//! let result = runtime.block_on(async {
//!     let instance = match instantiate(module, &store, &registry).await {
//!         VMResult::Success(instance) => instance,
//!         failure => panic!("instantiation failed: {failure:?}"),
//!     };
//!     telomere::run_module_function(
//!         &instance,
//!         &store,
//!         "add",
//!         &ResultValue::new(vec![WasmValue::I32(20), WasmValue::I32(22)]),
//!     )
//!     .await
//! });
//! match result {
//!     VMResult::Success(values) => {
//!         assert_eq!(values, ResultValue::new(vec![WasmValue::I32(42)]));
//!     }
//!     failure => panic!("guest call failed: {failure:?}"),
//! }
//! ```
//!
//! For a complete standalone crate with feature-ladder commands, see the
//! [minimal embedder](https://github.com/yieldspace/telomere/tree/main/examples/minimal-embedder).

/// Binary parsing internals are not yet covered by the embedder rustdoc pass.
/// TODO(#142-followup): document the binary-reader API.
#[allow(missing_docs)]
pub mod binary;
/// Core runtime internals remain intentionally outside the focused embedder surface.
/// TODO(#142-followup): document the remaining public common types.
#[allow(missing_docs)]
#[macro_use]
pub mod common;
/// Component-model adapters form the documented boundary used by component crates.
#[warn(missing_docs)]
pub mod component_support;
/// Parser internals are documented separately from the embedder-facing API.
/// TODO(#142-followup): document the remaining parser surface.
#[allow(missing_docs)]
pub mod parser;
/// Interpreter and scheduler internals remain outside this focused documentation pass.
/// TODO(#142-followup): document the remaining runtime surface.
#[allow(missing_docs)]
pub mod runtime;

/// Maximum nesting depth for `block`, `loop`, and `if` instructions accepted by [`WasmParser`].
///
/// Inputs that exceed this limit return [`WasmParserError::NestingTooDeep`].
pub const MAX_CONTROL_NESTING_DEPTH: u32 = 512;

/// Reads core WebAssembly bytes from any value that implements [`std::io::Read`].
pub use binary::IoReadBinaryReader;
/// Parsed runtime instance data; embedders normally retain an [`InstanceHandle`](common::InstanceHandle) instead.
pub use common::Instance;
/// Configures the optional lazy baseline JIT for a [`Store`].
pub use common::JitConfig;
/// Sets resource limits for memories created by a [`Store`].
pub use common::MemoryConfig;
/// The parsed, validated representation supplied to [`instantiate`].
pub use common::Module;
/// Resolves imported module names to already-instantiated instances.
pub use common::Registry;
/// Ordered values passed to, or returned from, an exported WebAssembly function.
pub use common::ResultValue;
/// Groups resource and execution settings for a [`Store`].
pub use common::RuntimeConfig;
/// The interpreter stack type, exposed for advanced embedding integrations.
pub use common::Stack;
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
/// Instantiates a native module whose host functions return futures.
pub use runtime::instantiate_native_async_module;
/// Reports whether this build and target can execute the optional JIT.
pub use runtime::jit_supported;
/// Replaces an exported guest function with an asynchronous host callback.
pub use runtime::link_async_host_function_with_export_name;
/// Replaces a guest function by index with an asynchronous host callback.
pub use runtime::link_async_host_function_with_function_idx;
/// Replaces an exported guest function with a synchronous host callback.
pub use runtime::link_host_function_with_export_name;
/// Replaces a guest function by index with a synchronous host callback.
pub use runtime::link_host_function_with_function_idx;
/// Calls a named exported function using the default Tokio-backed driver.
pub use runtime::run_module_function;
/// Calls a named exported function using an embedder-provided async driver.
pub use runtime::run_module_function_with_driver;
/// Marks the current host call as returning through the runtime's special path.
pub use runtime::special_function_return;
/// A completion delivered by an [`ExecutionDriver`].
pub use runtime::Completion;
/// The result payload carried by a [`Completion`].
pub use runtime::CompletionPayload;
/// Integrates pending WebAssembly operations with an embedder's async executor.
pub use runtime::ExecutionDriver;
/// A pending asynchronous host-function invocation.
pub use runtime::HostCallPending;
#[cfg(feature = "threads")]
/// A pending wait on shared WebAssembly memory.
pub use runtime::MemoryWaitPending;
/// Work submitted by the runtime to an [`ExecutionDriver`].
pub use runtime::PendingOp;
/// The default driver for async host calls and shared-memory waits.
pub use runtime::TokioDriver;
/// A reserved pending operation for future guest async support.
pub use runtime::WasmAsyncPending;
