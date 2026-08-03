use super::InterruptReason;

#[derive(Debug, Clone)]
#[must_use]
/// The result of instantiation or guest execution.
///
/// Unlike [`Result`], failures are WebAssembly traps and runtime conditions
/// rather than host-language errors. Match the variants to decide whether a
/// guest result can be used.
pub enum VMResult<V> {
    /// The operation completed and produced its value.
    Success(V),
    /// Guest execution reached an `unreachable` instruction.
    Unreachable,
    /// The guest exhausted the interpreter's stack capacity.
    StackOverflow,
    /// A guest memory instruction referenced a missing memory.
    MemoryIndexOutOfRange,
    /// A guest atomic instruction used an address with insufficient alignment.
    UnalignedAtomic,
    /// A guest table instruction referenced a missing table.
    TableIndexOutOfRange,
    /// An indirect call's runtime type did not match its expected type.
    CallIndirectInvalidType,
    /// An indirect call selected an uninitialized table entry.
    TableUninitialized,
    /// Imports, exports, or initialization could not be linked into an instance.
    Unlinkable,
    /// The runtime could not reserve or grow guest memory.
    MemoryAllocationFailed,
    /// A guest instruction received an invalid dynamic operand.
    InvalidOperand,
    /// The runtime does not implement this execution path yet.
    Unimplemented,
    /// Finite Store fuel was exhausted at a metering checkpoint.
    FuelExhausted,
    /// A watchdog requested Store execution cancellation at a metering checkpoint.
    Cancelled,
}
#[macro_export]
/// Propagates a non-success [`VMResult`](crate::VMResult) from the current function.
macro_rules! vm_try {
    ($expr: expr) => {
        match $expr {
            VMResult::Success(v) => v,
            VMResult::Unreachable => return VMResult::Unreachable,
            VMResult::StackOverflow => return VMResult::StackOverflow,
            VMResult::MemoryIndexOutOfRange => return VMResult::MemoryIndexOutOfRange,
            VMResult::UnalignedAtomic => return VMResult::UnalignedAtomic,
            VMResult::TableIndexOutOfRange => return VMResult::TableIndexOutOfRange,
            VMResult::CallIndirectInvalidType => return VMResult::CallIndirectInvalidType,
            VMResult::TableUninitialized => return VMResult::TableUninitialized,
            VMResult::Unlinkable => return VMResult::Unlinkable,
            VMResult::MemoryAllocationFailed => return VMResult::MemoryAllocationFailed,
            VMResult::InvalidOperand => return VMResult::InvalidOperand,
            VMResult::Unimplemented => return VMResult::Unimplemented,
            VMResult::FuelExhausted => return VMResult::FuelExhausted,
            VMResult::Cancelled => return VMResult::Cancelled,
        }
    };
}
impl<V> VMResult<V> {
    /// Returns the interruption reason when this result stopped at a metering checkpoint.
    pub fn interrupt_reason(&self) -> Option<InterruptReason> {
        match self {
            VMResult::FuelExhausted => Some(InterruptReason::FuelExhausted),
            VMResult::Cancelled => Some(InterruptReason::Cancelled),
            _ => None,
        }
    }

    /// Converts an option into a success or a caller-selected runtime failure.
    pub fn from_option(opt: Option<V>, err: impl FnOnce() -> VMResult<V>) -> VMResult<V> {
        match opt {
            Some(v) => VMResult::Success(v),
            None => err(),
        }
    }
    /// Returns the success value, panicking with the trap name for every failure variant.
    pub fn unwrap(self) -> V {
        match self {
            VMResult::Success(v) => v,
            VMResult::Unreachable => {
                panic!("called `VMResult::unwrap()` on an `Err` value: Unreachable",)
            }
            VMResult::StackOverflow => {
                panic!("called `VMResult::unwrap()` on an `Err` value: StackOverflow",)
            }
            VMResult::MemoryIndexOutOfRange => {
                panic!("called `VMResult::unwrap()` on an `Err` value: MemoryIndexOutOfRange",)
            }
            VMResult::UnalignedAtomic => {
                panic!("called `VMResult::unwrap()` on an `Err` value: UnalignedAtomic")
            }
            VMResult::TableIndexOutOfRange => {
                panic!("called `VMResult::unwrap()` on an `Err` value: TableIndexOutOfRange",)
            }
            VMResult::CallIndirectInvalidType => {
                panic!("called `VMResult::unwrap()` on an `Err` value: CallIndirectInvalidType",)
            }
            VMResult::TableUninitialized => {
                panic!("called `VMResult::unwrap()` on an `Err` value: TableUninitialized")
            }
            VMResult::Unlinkable => {
                panic!("called `VMResult::unwrap()` on an `Err` value: Unlinkable")
            }
            VMResult::MemoryAllocationFailed => {
                panic!("called `VMResult::unwrap()` on an `Err` value: MemoryAllocationFailed")
            }
            VMResult::InvalidOperand => {
                panic!("called `VMResult::unwrap()` on an `Err` value: InvalidOperand")
            }
            VMResult::Unimplemented => {
                panic!("called `VMResult::unwrap()` on an `Err` value: Unimplemented")
            }
            VMResult::FuelExhausted => {
                panic!("called `VMResult::unwrap()` on an `Err` value: FuelExhausted")
            }
            VMResult::Cancelled => {
                panic!("called `VMResult::unwrap()` on an `Err` value: Cancelled")
            }
        }
    }
    /// Returns `true` when this value is a trap or runtime failure.
    pub fn is_err(&self) -> bool {
        !matches!(self, VMResult::Success(_))
    }
}

impl InterruptReason {
    /// Converts this public interruption reason into the corresponding VM result variant.
    pub fn into_vm_result<V>(self) -> VMResult<V> {
        match self {
            InterruptReason::FuelExhausted => VMResult::FuelExhausted,
            InterruptReason::Cancelled => VMResult::Cancelled,
        }
    }
}
