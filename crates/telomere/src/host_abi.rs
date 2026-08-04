//! Raw ABI needed by Telomere's default host-linking capability.
//!
//! This module exposes selected interpreter representation because synchronous
//! and asynchronous host-function linking are advertised by the support matrix
//! and the rustdoc embedding examples in default builds. These items are a
//! compatibility carve-out rather than a newly designed stable API; issue #216
//! tracks replacing and closing the carve-out.
//!
//! # Example
//!
//! A synchronous callback can replace an exported guest function, write its
//! result into the return slot, and resume the guest through the raw stack ABI.
//!
//! ```
//! use futures::executor::block_on;
//! use telomere::{
//!     host_abi::{ExecuteContext, Instr},
//!     instantiate, link_host_function_with_export_name, run_module_function,
//!     IoReadBinaryReader, Registry, ResultValue, Store, VMResult, WasmParser, WasmValue,
//! };
//!
//! fn return_42(ctx: &mut ExecuteContext) -> VMResult<*const Instr> {
//!     ctx.return_slot().write(&42_i32.to_le_bytes());
//!     let (previous_local_reference, return_addr) = ctx
//!         .stack
//!         .function_return_in_place(&ctx.local_reference, 4, ctx.gc);
//!     ctx.set_local_reference(previous_local_reference);
//!     VMResult::Success(return_addr)
//! }
//!
//! let bytes = wat::parse_str(r#"(module (func (export "answer") (result i32) i32.const 0))"#)
//!     .expect("valid module");
//! let mut reader = IoReadBinaryReader::from(&bytes[..]);
//! let module = WasmParser::new(&mut reader)
//!     .parse_module()
//!     .expect("module parses");
//! let store = Store::new();
//! let registry = Registry::new();
//! let instance = match block_on(instantiate(module, &store, &registry)) {
//!     VMResult::Success(instance) => instance,
//!     failure => panic!("instantiation failed: {failure:?}"),
//! };
//! link_host_function_with_export_name(&instance, "answer", return_42, &store);
//! let result = block_on(run_module_function(
//!     &instance,
//!     &store,
//!     "answer",
//!     &ResultValue::new(vec![]),
//! ));
//! assert_eq!(
//!     result.unwrap(),
//!     ResultValue::new(vec![WasmValue::I32(42)])
//! );
//! ```
//!
//! # Async driver example
//!
//! An embedder can define an asynchronous host callback, create a native module,
//! link it by index and export name, and drive the returned host future through
//! a custom [`crate::ExecutionDriver`].
//!
//! ```
//! use std::{collections::VecDeque, future::Future, pin::Pin};
//!
//! use futures::executor::block_on;
//! use telomere::{
//!     component_support::common::{FuncType, ValType},
//!     host_abi::{AsyncHostFunctionDefinition, AsyncHostFuture, AsyncNativeModule, ExecuteContext},
//!     instantiate, instantiate_native_async_module, link_async_host_function_with_export_name,
//!     link_async_host_function_with_function_idx, run_module_function_with_driver, Completion,
//!     CompletionPayload, ExecutionDriver, HostCallPending, IoReadBinaryReader, PendingOp, Registry,
//!     ResultValue, Store, TokioDriver, VMResult, WasmParser, WasmValue,
//! };
//!
//! #[derive(Default)]
//! struct InlineDriver {
//!     inflight: VecDeque<(u32, AsyncHostFuture)>,
//! }
//!
//! impl ExecutionDriver for InlineDriver {
//!     fn submit(&mut self, op: PendingOp) {
//!         match op {
//!             PendingOp::HostCall(HostCallPending { task_id, future }) => {
//!                 self.inflight.push_back((task_id, future));
//!             }
//!             _ => panic!("this example only drives asynchronous host calls"),
//!         }
//!     }
//!
//!     fn next_completion<'a>(
//!         &'a mut self,
//!     ) -> Pin<Box<dyn Future<Output = Option<Completion>> + 'a>> {
//!         Box::pin(async move {
//!             let (task_id, future) = self.inflight.pop_front()?;
//!             Some(Completion {
//!                 task_id,
//!                 payload: CompletionPayload::HostCall {
//!                     result: future.await,
//!                 },
//!             })
//!         })
//!     }
//! }
//!
//! fn return_42(ctx: &mut ExecuteContext<'_>) -> AsyncHostFuture {
//!     let slot = ctx.return_slot();
//!     let (previous_local_reference, return_addr) =
//!         ctx.stack
//!             .function_return_in_place(&ctx.local_reference, 4, ctx.gc);
//!     ctx.set_local_reference(previous_local_reference);
//!     Box::pin(async move {
//!         slot.write(&42_i32.to_le_bytes());
//!         VMResult::Success(return_addr)
//!     })
//! }
//!
//! let _built_in_driver = TokioDriver::new();
//! let bytes = wat::parse_str(
//!     r#"(module
//!         (import "host" "answer" (func $answer (result i32)))
//!         (func (export "run") (result i32) call $answer))"#,
//! )
//! .expect("the inline module is valid WebAssembly");
//! let mut reader = IoReadBinaryReader::from(&bytes[..]);
//! let module = WasmParser::new(&mut reader)
//!     .parse_module()
//!     .expect("the module parses");
//! let store = Store::new();
//! let mut registry = Registry::new();
//! let host = block_on(instantiate_native_async_module(
//!     AsyncNativeModule {
//!         functions: vec![AsyncHostFunctionDefinition {
//!             name: Some("answer".to_owned()),
//!             signature: FuncType::new(vec![], vec![ValType::I32]),
//!             fp: return_42,
//!         }],
//!     },
//!     &store,
//!     &registry,
//! ))
//! .unwrap();
//! link_async_host_function_with_function_idx(&host, 0, return_42, &store);
//! link_async_host_function_with_export_name(&host, "answer", return_42, &store);
//! registry.register("host", host);
//! let instance = block_on(instantiate(module, &store, &registry)).unwrap();
//! let mut driver = InlineDriver::default();
//! let result = block_on(run_module_function_with_driver(
//!     &instance,
//!     &store,
//!     "run",
//!     &ResultValue::new(vec![]),
//!     &mut driver,
//! ));
//! assert_eq!(
//!     result.unwrap(),
//!     ResultValue::new(vec![WasmValue::I32(42)])
//! );
//! ```

/// Shared guest memory passed to a custom asynchronous execution driver.
///
/// When the optional `threads` runtime is enabled, the public
/// [`crate::MemoryWaitPending`] contract carries this representation so a
/// driver can await shared-memory operations. Issue #216 tracks replacing this
/// compatibility ABI.
#[cfg(feature = "threads")]
pub use crate::common::memory::SharedMemoryObject;
/// A registration for a pending shared-memory wait.
///
/// When the optional `threads` runtime is enabled, custom execution drivers
/// consume this representation from [`crate::MemoryWaitPending`] and call
/// [`SharedWaitRegistration::wait_result`]. Issue #216 tracks replacing this
/// compatibility ABI.
#[cfg(feature = "threads")]
pub use crate::common::memory::SharedWaitRegistration;
/// A raw reference to the active interpreter call frame's local area.
///
/// Default host callbacks receive and update this representation while
/// returning from a guest frame. Issue #216 tracks replacing this compatibility
/// ABI.
pub use crate::common::stack::LocalReference;
/// The raw interpreter stack used by host callbacks.
///
/// Default host linking requires callbacks to marshal values and complete a
/// frame return through the running stack. Issue #216 tracks replacing this
/// compatibility ABI.
pub use crate::common::stack::Stack;
/// The raw mutable store runtime used to complete a host callback frame.
///
/// Default host-linking callbacks pass this representation to stack return
/// operations. Issue #216 tracks replacing this compatibility ABI.
pub use crate::common::store::StoreInner;
/// The asynchronous host callback signature.
///
/// It exposes the interpreter context because asynchronous host linking is
/// available in default builds. Issue #216 tracks its replacement.
pub use crate::common::AsyncHostFunction;
/// An asynchronous host callback and its WebAssembly signature.
///
/// It exposes raw callback representation for default host linking; issue #216
/// tracks replacing this compatibility ABI.
pub use crate::common::AsyncHostFunctionDefinition;
/// The future returned by an asynchronous host callback.
///
/// It exposes an interpreter continuation because asynchronous host linking is
/// available in default builds. Issue #216 tracks its replacement.
pub use crate::common::AsyncHostFuture;
/// A synthetic module composed of asynchronous host callbacks.
///
/// It exposes raw native-module representation for default host linking; issue
/// #216 tracks replacing this compatibility ABI.
pub use crate::common::AsyncNativeModule;
/// The decoded function-body section of a module.
///
/// It remains public through [`crate::Module::codes`] so default host-linking
/// integrations retain their documented AST access. Issue #216 tracks its
/// replacement.
pub use crate::common::CodeSection;
/// The raw interpreter context supplied to host callbacks.
///
/// It exposes execution state because default synchronous and asynchronous host
/// linking require callbacks to interact with a running guest. Issue #216
/// tracks replacing this public compatibility ABI.
pub use crate::common::ExecuteContext;
/// The decoded body of a WebAssembly function.
///
/// It remains nameable through [`CodeSection`] for default host-linking AST
/// compatibility. Issue #216 tracks its replacement.
pub use crate::common::Func;
/// A decoded WebAssembly or native-host function body.
///
/// It remains public through [`CodeSection`] for default host-linking AST
/// compatibility. Issue #216 tracks its replacement.
pub use crate::common::FunctionBody;
/// The synchronous host callback signature.
///
/// It exposes the interpreter context and continuation because synchronous host
/// linking is available in default builds. Issue #216 tracks its replacement.
pub use crate::common::HostFunction;
/// A synchronous host callback and its WebAssembly signature.
///
/// It exposes raw callback representation for default host linking; issue #216
/// tracks replacing this compatibility ABI.
pub use crate::common::HostFunctionDefinition;
/// The raw instruction record used as a host callback continuation.
///
/// It is public because default host linking returns interpreter continuations;
/// issue #216 tracks replacing this compatibility ABI.
pub use crate::common::Instr;
/// A raw local-memory allocation owned by the active interpreter instance.
///
/// Default host linking exposes this representation through
/// [`ExecuteContext::memory`](crate::host_abi::ExecuteContext::memory), so
/// callbacks can read and write guest memory. Issue #216 tracks replacing this
/// compatibility ABI.
pub use crate::common::Memory;
/// A synthetic module composed of synchronous host callbacks.
///
/// It exposes raw native-module representation for default host linking; issue
/// #216 tracks replacing this compatibility ABI.
pub use crate::common::NativeModule;
/// A raw reference into Telomere's store object arena.
///
/// It remains nameable because default host-linking context APIs expose object
/// references. Issue #216 tracks replacing this compatibility ABI.
pub use crate::common::ObjectRef;
/// The in-place result area exposed to synchronous and asynchronous host callbacks.
///
/// Default host linking writes results through this raw return slot. Issue #216
/// tracks replacing this compatibility ABI.
pub use crate::common::ReturnSlot;
/// Instantiates a synthetic module composed of synchronous host callbacks.
///
/// It exposes native-module construction for default host linking; issue #216
/// tracks replacing this compatibility ABI.
pub use crate::runtime::instantiate::instantiate_native_module;
