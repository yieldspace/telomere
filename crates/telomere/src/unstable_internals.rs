//! Unstable interpreter internals for downstream integrations that explicitly opt in.
//!
//! This module is gated by the `unstable-internals` feature and makes no stability promise.
//! Its items expose the interpreter's internal representation and may change in any release.
//! Host-function semantics are owned by #147 and #126, and Preview1's fate is tracked by #137.

/// The interpreter dispatch-handler signature.
pub use crate::common::Op;
/// The interpreter instruction operand representation.
pub use crate::common::Operand;
/// The interpreter value-width representation.
pub use crate::common::ValueSize;

/// Returns the producer-supplied module name from a parsed `name` custom section.
///
/// This feature-gated accessor exposes only the retained value, not the
/// parser's custom-section representation.
pub fn module_name(module: &crate::Module) -> Option<&str> {
    module
        .name
        .as_ref()
        .and_then(|names| names.module_name.as_ref())
        .map(|name| name.0.as_str())
}

/// Returns the number of asynchronous effects queued by the current callback.
///
/// This is a feature-gated replacement for inspecting the raw effect supplier.
pub fn pending_effect_count(context: &crate::host_abi::ExecuteContext<'_>) -> u32 {
    context.effect.get_pending_count()
}

/// Returns the core-function index for the callback currently being executed.
///
/// This is a feature-gated replacement for inspecting raw function-instance
/// metadata.
pub fn current_function_index(context: &crate::host_abi::ExecuteContext<'_>) -> u32 {
    context.func().funcidx
}

/// Returns the raw function reference at `index` in the current instance.
///
/// This is a feature-gated replacement for inspecting raw instance metadata.
pub fn function_address(
    context: &crate::host_abi::ExecuteContext<'_>,
    index: usize,
) -> Option<crate::host_abi::ObjectRef> {
    context.instance().funcs.get(index).copied()
}

/// Reports whether `address` denotes a host or asynchronous-host function.
///
/// `address` must be a function reference owned by the context's store.
pub fn function_is_host(
    context: &crate::host_abi::ExecuteContext<'_>,
    address: crate::host_abi::ObjectRef,
) -> bool {
    context.func_by_addr(address).is_host_func()
}

/// Returns the synchronous host callback at `address`, when it has one.
///
/// `address` must be a function reference owned by the context's store.
pub fn function_host_code_pointer(
    context: &crate::host_abi::ExecuteContext<'_>,
    address: crate::host_abi::ObjectRef,
) -> Option<crate::host_abi::HostFunction> {
    let function = context.func_by_addr(address);
    (function.is_host_func() && !function.is_async_host_func())
        .then(|| function.host_code_pointer())
}

/// Returns the byte size of the locals required by the function at `address`.
///
/// `address` must be a function reference owned by the context's store.
pub fn function_locals_size(
    context: &crate::host_abi::ExecuteContext<'_>,
    address: crate::host_abi::ObjectRef,
) -> usize {
    context.func_by_addr(address).locals().byte_size()
}

/// Returns the first interpreter instruction for a WebAssembly function.
///
/// Returns `None` for host functions. `address` must be a function reference
/// owned by the context's store.
pub fn function_code_pointer(
    context: &crate::host_abi::ExecuteContext<'_>,
    address: crate::host_abi::ObjectRef,
) -> Option<*const crate::host_abi::Instr> {
    context.func_by_addr(address).code_pointer()
}

/// Opens a new interpreter call frame for a function reference.
///
/// This feature-gated helper owns the raw call-frame cache and preserves the
/// current local reference and store association. `frame` must be a function
/// reference owned by the context's store.
pub fn function_call(
    context: &mut crate::host_abi::ExecuteContext<'_>,
    param_size: usize,
    local_size: usize,
    frame: crate::host_abi::ObjectRef,
    return_addr: *const crate::host_abi::Instr,
) -> crate::VMResult<crate::host_abi::LocalReference> {
    let previous_local_reference = context.local_reference;
    let runtime = &*context.gc;
    context.stack.function_call(
        param_size,
        local_size,
        frame,
        previous_local_reference,
        return_addr,
        runtime,
    )
}

/// An atomic read-modify-write operation accepted by the shared-memory helpers.
///
/// This wrapper avoids exposing Telomere's internal atomic-operation enum.
#[cfg(feature = "threads")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtomicRmwOperation {
    /// Add the supplied value with wrapping arithmetic.
    Add,
    /// Subtract the supplied value with wrapping arithmetic.
    Sub,
    /// Bitwise-and the supplied value.
    And,
    /// Bitwise-or the supplied value.
    Or,
    /// Bitwise-xor the supplied value.
    Xor,
    /// Replace the stored value with the supplied value.
    Xchg,
}

#[cfg(feature = "threads")]
impl AtomicRmwOperation {
    fn into_internal(self) -> crate::common::AtomicRmwOp {
        match self {
            Self::Add => crate::common::AtomicRmwOp::Add,
            Self::Sub => crate::common::AtomicRmwOp::Sub,
            Self::And => crate::common::AtomicRmwOp::And,
            Self::Or => crate::common::AtomicRmwOp::Or,
            Self::Xor => crate::common::AtomicRmwOp::Xor,
            Self::Xchg => crate::common::AtomicRmwOp::Xchg,
        }
    }
}

/// The observable result of attempting to register a shared-memory wait.
///
/// This wrapper avoids exposing Telomere's internal atomic-wait result enum.
#[cfg(feature = "threads")]
#[derive(Debug)]
pub enum SharedWaitState {
    /// The memory value did not equal the requested wait value.
    NotEqual,
    /// The wait was registered and can be driven through the public ABI.
    Pending(crate::host_abi::SharedWaitRegistration),
}

/// Creates shared memory for an unstable integration or low-level test.
///
/// Allocation details remain internal; failures are reported as
/// [`crate::VMResult::MemoryAllocationFailed`].
#[cfg(feature = "threads")]
pub fn new_shared_memory(
    page_count: u32,
    max_page_size: u32,
) -> crate::VMResult<std::sync::Arc<crate::host_abi::SharedMemoryObject>> {
    match crate::common::memory::SharedMemoryObject::new(page_count, max_page_size) {
        Ok(memory) => crate::VMResult::Success(memory),
        Err(_) => crate::VMResult::MemoryAllocationFailed,
    }
}

/// Applies an atomic read-modify-write operation to an eight-bit shared value.
#[cfg(feature = "threads")]
pub fn shared_atomic_rmw_u8(
    memory: &crate::host_abi::SharedMemoryObject,
    offset: usize,
    operation: AtomicRmwOperation,
    value: u8,
) -> crate::VMResult<u8> {
    memory.atomic_rmw_u8(offset, operation.into_internal(), value)
}

/// Applies an atomic read-modify-write operation to a sixteen-bit shared value.
#[cfg(feature = "threads")]
pub fn shared_atomic_rmw_u16(
    memory: &crate::host_abi::SharedMemoryObject,
    offset: usize,
    operation: AtomicRmwOperation,
    value: u16,
) -> crate::VMResult<u16> {
    memory.atomic_rmw_u16(offset, operation.into_internal(), value)
}

/// Applies an atomic read-modify-write operation to a thirty-two-bit shared value.
#[cfg(feature = "threads")]
pub fn shared_atomic_rmw_u32(
    memory: &crate::host_abi::SharedMemoryObject,
    offset: usize,
    operation: AtomicRmwOperation,
    value: u32,
) -> crate::VMResult<u32> {
    memory.atomic_rmw_u32(offset, operation.into_internal(), value)
}

/// Applies an atomic read-modify-write operation to a sixty-four-bit shared value.
#[cfg(feature = "threads")]
pub fn shared_atomic_rmw_u64(
    memory: &crate::host_abi::SharedMemoryObject,
    offset: usize,
    operation: AtomicRmwOperation,
    value: u64,
) -> crate::VMResult<u64> {
    memory.atomic_rmw_u64(offset, operation.into_internal(), value)
}

/// Registers a wait for a thirty-two-bit shared value.
#[cfg(feature = "threads")]
pub fn shared_register_wait32(
    memory: &crate::host_abi::SharedMemoryObject,
    offset: usize,
    expected: u32,
) -> crate::VMResult<SharedWaitState> {
    map_vm_result(
        memory.register_wait32(offset, expected),
        |result| match result {
            crate::common::AtomicWaitResult::NotEqual => SharedWaitState::NotEqual,
            crate::common::AtomicWaitResult::Pending(wait) => SharedWaitState::Pending(wait),
        },
    )
}

/// Registers a wait for a sixty-four-bit shared value.
#[cfg(feature = "threads")]
pub fn shared_register_wait64(
    memory: &crate::host_abi::SharedMemoryObject,
    offset: usize,
    expected: u64,
) -> crate::VMResult<SharedWaitState> {
    map_vm_result(
        memory.register_wait64(offset, expected),
        |result| match result {
            crate::common::AtomicWaitResult::NotEqual => SharedWaitState::NotEqual,
            crate::common::AtomicWaitResult::Pending(wait) => SharedWaitState::Pending(wait),
        },
    )
}

#[cfg(feature = "threads")]
fn map_vm_result<T, U>(result: crate::VMResult<T>, map: impl FnOnce(T) -> U) -> crate::VMResult<U> {
    match result {
        crate::VMResult::Success(value) => crate::VMResult::Success(map(value)),
        crate::VMResult::Unreachable => crate::VMResult::Unreachable,
        crate::VMResult::StackOverflow => crate::VMResult::StackOverflow,
        crate::VMResult::MemoryIndexOutOfRange => crate::VMResult::MemoryIndexOutOfRange,
        crate::VMResult::UnalignedAtomic => crate::VMResult::UnalignedAtomic,
        crate::VMResult::TableIndexOutOfRange => crate::VMResult::TableIndexOutOfRange,
        crate::VMResult::CallIndirectInvalidType => crate::VMResult::CallIndirectInvalidType,
        crate::VMResult::TableUninitialized => crate::VMResult::TableUninitialized,
        crate::VMResult::Unlinkable => crate::VMResult::Unlinkable,
        crate::VMResult::MemoryAllocationFailed => crate::VMResult::MemoryAllocationFailed,
        crate::VMResult::InvalidOperand => crate::VMResult::InvalidOperand,
        crate::VMResult::Unimplemented => crate::VMResult::Unimplemented,
        crate::VMResult::FuelExhausted => crate::VMResult::FuelExhausted,
        crate::VMResult::Cancelled => crate::VMResult::Cancelled,
    }
}
