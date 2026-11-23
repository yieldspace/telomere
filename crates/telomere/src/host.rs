//! The designed public host-function boundary.
//!
//! Host functions are written against public vocabulary only: they receive a
//! [`HostContext`] carrying the decoded arguments, the owning [`Store`], and
//! guest-memory access, and they return their results as [`WasmValue`]s (or a
//! trap through [`VMResult`]). The interpreter keeps its internal calling
//! convention behind this boundary; the adapter in this module decodes the
//! arguments from the frame's local area, validates and writes the results
//! into the frame's return slot, and completes the frame return, so the
//! adaptation cost is paid once here rather than in every embedder.
//!
//! The interpreter-facing return of every adapter is the same
//! `VMResult<*const Instr>` the raw internal convention uses, so the
//! host-call dispatch path's return-type classification (guarded by the
//! size assertions tracked in issue #178) is unchanged by this boundary.

use std::{future::Future, pin::Pin};

use crate::{
    common::{ExecuteContext, FuncType, Instr, Memory, ResultType, ValType, WasmValue},
    runtime::memory_effect::HostCallResult,
    Store, VMResult,
};

/// The context supplied to a host function while a guest call is in flight.
///
/// A `HostContext` is valid only for the duration of the host call that
/// received it: the arguments are decoded copies, and the memory accessors
/// borrow the calling frame's memory for that call.
pub struct HostContext<'ctx, 'env> {
    ctx: &'ctx mut ExecuteContext<'env>,
    args: Vec<WasmValue>,
}

impl<'ctx, 'env> HostContext<'ctx, 'env> {
    pub(crate) fn new(ctx: &'ctx mut ExecuteContext<'env>, args: Vec<WasmValue>) -> Self {
        Self { ctx, args }
    }

    /// The arguments the guest passed to this host function, decoded
    /// according to the function's declared signature.
    pub fn args(&self) -> &[WasmValue] {
        &self.args
    }

    /// The store that owns the calling instance.
    ///
    /// Embedder state installed with [`Store::new_with_state`] is reachable
    /// through `store.state`, exactly as in the rest of the embedding API.
    pub fn store(&self) -> &Store {
        self.ctx.store
    }

    /// The store-local identifier of the instance that owns the called
    /// function.
    ///
    /// Together with [`func_index`](Self::func_index) this identifies which
    /// linked function was invoked, for embedders that register one callback
    /// for several functions.
    pub fn instance_id(&self) -> u32 {
        self.ctx.instance_id()
    }

    /// The index, within its owning instance, of the function that was
    /// called.
    pub fn func_index(&self) -> u32 {
        self.ctx.func().funcidx
    }

    /// The default memory of the instance that owns the called function, if
    /// that instance has one.
    pub fn memory(&mut self) -> Option<&mut Memory> {
        self.ctx.memory()
    }

    /// Runs `f` against the default memory of the instance that owns the
    /// called function, if that instance has one, and returns its result.
    ///
    /// Unlike [`memory`](Self::memory), this accessor also reaches a
    /// **shared** default memory: a shared memory cannot be handed out as a
    /// plain `&mut Memory`, so the closure runs while the runtime holds the
    /// shared memory's lock.
    pub fn with_memory<T>(&mut self, f: impl FnOnce(&mut Memory) -> T) -> Option<T> {
        self.ctx.with_memory(f)
    }

    /// The default memory of the *calling* instance, if it has one.
    ///
    /// Host functions linked into a native module usually want the caller's
    /// memory: it is the guest memory their arguments point into.
    pub fn caller_memory(&mut self) -> Option<&mut Memory> {
        self.ctx.caller_memory()
    }

    /// Runs `f` against the default memory of the *calling* instance, if it
    /// has one, and returns its result.
    ///
    /// Unlike [`caller_memory`](Self::caller_memory), this accessor also
    /// reaches a **shared** caller memory: a shared memory cannot be handed
    /// out as a plain `&mut Memory`, so the closure runs while the runtime
    /// holds the shared memory's lock. Host functions linked into a native
    /// module usually want the caller's memory: it is the guest memory
    /// their arguments point into.
    pub fn with_caller_memory<T>(&mut self, f: impl FnOnce(&mut Memory) -> T) -> Option<T> {
        self.ctx.with_caller_memory(f)
    }
}

/// A synchronous host function written against the public boundary.
///
/// The function receives the decoded arguments through [`HostContext`] and
/// returns the values the guest observes, in declaration order, or a
/// [`VMResult`] trap. Returning the wrong number or type of values traps
/// with [`VMResult::InvalidOperand`].
pub type HostFn = fn(&mut HostContext<'_, '_>) -> VMResult<Vec<WasmValue>>;

/// The future returned by an asynchronous host function.
///
/// It resolves to the values the guest observes, in declaration order, or a
/// [`VMResult`] trap. The future must not borrow the [`HostContext`]: guest
/// execution suspends while it is pending and resumes when it completes.
pub type HostFuture = Pin<Box<dyn Future<Output = VMResult<Vec<WasmValue>>> + 'static>>;

/// An asynchronous host function written against the public boundary.
///
/// The callback runs while the guest frame is live, so it can read its
/// arguments and memory synchronously; the returned [`HostFuture`] is then
/// driven by the runtime's execution driver (see
/// [`run_module_function_with_driver`](crate::run_module_function_with_driver)).
pub type AsyncHostFn = fn(&mut HostContext<'_, '_>) -> HostFuture;

/// A synchronous host function and its WebAssembly signature, for inclusion
/// in a [`HostModule`].
pub struct HostFnDefinition {
    /// Optional export name assigned to the generated native module.
    pub name: Option<String>,
    /// Parameter and result types accepted by the host callback.
    pub signature: FuncType,
    /// Synchronous callback invoked by the interpreter.
    pub fp: HostFn,
}

/// An asynchronous host function and its WebAssembly signature, for
/// inclusion in an [`AsyncHostModule`].
pub struct AsyncHostFnDefinition {
    /// Optional export name assigned to the generated native module.
    pub name: Option<String>,
    /// Parameter and result types accepted by the host callback.
    pub signature: FuncType,
    /// Callback whose future completes before guest execution resumes.
    pub fp: AsyncHostFn,
}

/// A collection of synchronous host functions that can be instantiated as a
/// module with [`instantiate_native_module`](crate::instantiate_native_module).
pub struct HostModule {
    /// Host functions exposed by the native module.
    pub functions: Vec<HostFnDefinition>,
}

/// A collection of asynchronous host functions that can be instantiated as
/// a module with
/// [`instantiate_native_async_module`](crate::instantiate_native_async_module).
pub struct AsyncHostModule {
    /// Host functions exposed by the native module.
    pub functions: Vec<AsyncHostFnDefinition>,
}

/// Resolves the declared signature of the function the context is currently
/// executing, mirroring the lookup `build_call_recipe` performs.
fn current_signature(ctx: &ExecuteContext) -> FuncType {
    let func = ctx.func();
    let module = ctx.module();
    let typeidx = module.functions[func.funcidx as usize];
    module.function_types[typeidx.0 as usize].clone()
}

/// Decodes the arguments sitting at the start of the active frame's local
/// area. Parameters are laid out in declaration order at cumulative
/// stack-size offsets, with no extra padding.
fn decode_args(ctx: &ExecuteContext, params: &ResultType) -> Vec<WasmValue> {
    let local_reference = ctx.local_reference();
    let mut offset = 0usize;
    let mut args = Vec::with_capacity(params.0.len());
    for ty in params.iter() {
        let size = ty.stack_size().usize();
        let bytes = ctx.stack.local_bytes(&local_reference, offset, size);
        args.push(decode_value(*ty, bytes));
        offset += size;
    }
    args
}

fn decode_value(ty: ValType, bytes: &[u8]) -> WasmValue {
    match ty {
        ValType::I32 => WasmValue::I32(i32::from_le_bytes(bytes[..4].try_into().unwrap())),
        ValType::I64 => WasmValue::I64(i64::from_le_bytes(bytes[..8].try_into().unwrap())),
        ValType::F32 => WasmValue::F32(f32::from_le_bytes(bytes[..4].try_into().unwrap())),
        ValType::F64 => WasmValue::F64(f64::from_le_bytes(bytes[..8].try_into().unwrap())),
        ValType::V128 => WasmValue::V128(u128::from_le_bytes(bytes[..16].try_into().unwrap())),
        ValType::FuncRef => WasmValue::FuncRef(u32::from_le_bytes(bytes[..4].try_into().unwrap())),
        ValType::ExternRef => {
            WasmValue::ExternRef(u32::from_le_bytes(bytes[..4].try_into().unwrap()))
        }
    }
}

fn value_matches_type(ty: ValType, value: &WasmValue) -> bool {
    matches!(
        (ty, value),
        (ValType::I32, WasmValue::I32(_))
            | (ValType::I64, WasmValue::I64(_))
            | (ValType::F32, WasmValue::F32(_))
            | (ValType::F64, WasmValue::F64(_))
            | (ValType::V128, WasmValue::V128(_))
            | (ValType::FuncRef, WasmValue::FuncRef(_))
            | (ValType::ExternRef, WasmValue::ExternRef(_))
    )
}

/// Validates `values` against the declared result types and writes them, in
/// declaration order at cumulative stack-size offsets, into the return slot
/// at `slot`. Returns the total result size in bytes.
pub(crate) fn write_result_values(
    slot: *mut u8,
    results: &ResultType,
    values: &[WasmValue],
) -> VMResult<usize> {
    if values.len() != results.0.len() {
        return VMResult::InvalidOperand;
    }
    let mut offset = 0usize;
    for (ty, value) in results.iter().zip(values) {
        if !value_matches_type(*ty, value) {
            return VMResult::InvalidOperand;
        }
        let mut buf = [0u8; 16];
        let len = match value {
            WasmValue::I32(v) => {
                buf[..4].copy_from_slice(&v.to_le_bytes());
                4
            }
            WasmValue::I64(v) => {
                buf[..8].copy_from_slice(&v.to_le_bytes());
                8
            }
            WasmValue::F32(v) => {
                buf[..4].copy_from_slice(&v.to_le_bytes());
                4
            }
            WasmValue::F64(v) => {
                buf[..8].copy_from_slice(&v.to_le_bytes());
                8
            }
            WasmValue::V128(v) => {
                buf[..16].copy_from_slice(&v.to_le_bytes());
                16
            }
            WasmValue::FuncRef(v) | WasmValue::ExternRef(v) => {
                buf[..4].copy_from_slice(&v.to_le_bytes());
                4
            }
        };
        // SAFETY: the caller guarantees `slot` is the active frame's return
        // slot, whose capacity is the larger of the parameter and result
        // areas, and `offset + len` stays within the declared result size.
        unsafe {
            std::ptr::copy_nonoverlapping(buf.as_ptr(), slot.add(offset), len);
        }
        offset += len;
    }
    VMResult::Success(offset)
}

/// Adapter behind every synchronous public host function: decodes the
/// arguments, runs the public callback, writes its results into the return
/// slot, and completes the frame return with the interpreter's internal
/// calling convention.
pub(crate) fn invoke_public_host_function(
    ctx: &mut ExecuteContext,
    fp: HostFn,
) -> VMResult<*const Instr> {
    let signature = current_signature(ctx);
    let args = decode_args(ctx, &signature.0);
    let mut cx = HostContext::new(ctx, args);
    let values = match fp(&mut cx) {
        VMResult::Success(values) => values,
        other => return other.propagate_failure(),
    };
    let ctx = cx.ctx;
    let slot = ctx.return_slot().as_mut_ptr();
    let return_size = match write_result_values(slot, &signature.1, &values) {
        VMResult::Success(size) => size,
        other => return other.propagate_failure(),
    };
    let (previous_local_reference, return_addr) =
        ctx.stack
            .function_return_in_place(&ctx.local_reference, return_size, ctx.gc);
    ctx.set_local_reference(previous_local_reference);
    VMResult::Success(return_addr)
}

/// Adapter behind every asynchronous public host function: decodes the
/// arguments and runs the public callback while the frame is live, completes
/// the frame return eagerly (mirroring the raw async contract), and returns
/// a future that resolves to the callback's values.
///
/// The future deliberately does **not** write the values into the frame's
/// return slot: it is handed to an embedder-owned driver and may still be
/// polled after the execution that submitted it has been dropped, when the
/// task's stack no longer exists. The scheduler, which owns the stack,
/// validates the values against the declared result types and writes them
/// into the return slot when it applies the completion.
pub(crate) fn start_public_async_host_call(
    ctx: &mut ExecuteContext,
    fp: AsyncHostFn,
) -> VMResult<Pin<Box<dyn Future<Output = HostCallResult> + 'static>>> {
    let signature = current_signature(ctx);
    let args = decode_args(ctx, &signature.0);
    let mut cx = HostContext::new(ctx, args);
    let future = fp(&mut cx);
    let ctx = cx.ctx;
    let return_size = signature
        .1
        .iter()
        .map(|ty| ty.stack_size().usize())
        .sum::<usize>();
    let (previous_local_reference, return_addr) =
        ctx.stack
            .function_return_in_place(&ctx.local_reference, return_size, ctx.gc);
    ctx.set_local_reference(previous_local_reference);
    let result_types = signature.1.clone();
    let future = Box::pin(async move {
        HostCallResult::Values {
            result: future.await,
            return_addr,
            result_types,
        }
    });
    VMResult::Success(future)
}
