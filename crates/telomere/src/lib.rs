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

pub mod binary;
#[macro_use]
pub mod common;
pub mod component_support;
pub mod parser;
pub mod runtime;

pub use binary::IoReadBinaryReader;
pub use common::Instance;
pub use common::JitConfig;
pub use common::Module;
pub use common::Registry;
pub use common::ResultValue;
pub use common::RuntimeConfig;
pub use common::Stack;
pub use common::VMResult;
pub use common::WasmValue;
pub use common::{Store, StoreState};
pub use parser::core::ProposalFeature;
pub use parser::core::WasmParser;
pub use parser::core::WasmParserError;
pub use runtime::aliasing;
pub use runtime::get_global;
pub use runtime::instantiate;
pub use runtime::instantiate_native_async_module;
pub use runtime::jit_supported;
pub use runtime::link_async_host_function_with_export_name;
pub use runtime::link_async_host_function_with_function_idx;
pub use runtime::link_host_function_with_export_name;
pub use runtime::link_host_function_with_function_idx;
pub use runtime::run_module_function;
pub use runtime::run_module_function_with_driver;
pub use runtime::special_function_return;
pub use runtime::Completion;
pub use runtime::CompletionPayload;
pub use runtime::ExecutionDriver;
pub use runtime::HostCallPending;
#[cfg(feature = "threads")]
pub use runtime::MemoryWaitPending;
pub use runtime::PendingOp;
pub use runtime::TokioDriver;
pub use runtime::WasmAsyncPending;
