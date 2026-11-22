use super::InterruptReason;
use std::fmt;

// Adding a payload-carrying variant is subject to the hot-dispatch ABI size guards in
// `runtime/vm.rs`; see yieldspace/telomere#178.
#[derive(Debug, Clone)]
#[must_use]
/// The result of instantiation or guest execution.
///
/// Unlike [`Result`], failures are WebAssembly traps and runtime conditions
/// rather than host-language errors. Match the variants to decide whether a
/// guest result can be used.
///
/// The failure variants implement [`fmt::Display`] with the WebAssembly
/// spec's trap messages where the spec defines one, and `VMResult<V>`
/// implements [`std::error::Error`] when `V: fmt::Debug`, so a failed call can
/// flow through ordinary error handling. For the faulting function and a
/// captured frame trace, see [`Store::take_last_trap`](crate::Store::take_last_trap).
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
    /// A runtime-internal invariant was violated (for example, a decoded
    /// instruction carried an operand kind the validator guarantees cannot
    /// occur, or a store-internal link was missing).
    ///
    /// This is not a WebAssembly spec trap: validated guest execution cannot
    /// produce it. Spec trap causes that used to be reported as
    /// `InvalidOperand` are now reported precisely as
    /// [`DivideByZero`](Self::DivideByZero),
    /// [`IntegerOverflow`](Self::IntegerOverflow), or
    /// [`InvalidConversion`](Self::InvalidConversion).
    InvalidOperand,
    /// A guest integer division or remainder had a zero divisor
    /// (spec trap: "integer divide by zero").
    DivideByZero,
    /// A guest signed integer division overflowed, dividing the most
    /// negative value by -1, or a float-to-integer truncation conversion
    /// had an out-of-range magnitude, including infinities
    /// (spec trap: "integer overflow").
    IntegerOverflow,
    /// A guest float-to-integer truncation conversion received NaN
    /// (spec trap: "invalid conversion to integer").
    InvalidConversion,
    /// The runtime does not implement this execution path yet.
    Unimplemented,
    /// Finite Store fuel was exhausted at a metering checkpoint.
    FuelExhausted,
    /// A watchdog requested Store execution cancellation at a metering checkpoint.
    Cancelled,
}

impl<V> fmt::Display for VMResult<V> {
    /// Formats the failure with the WebAssembly spec's trap message where the
    /// spec defines one (for example, `DivideByZero` displays as
    /// "integer divide by zero"), so embedders and spec-test harnesses can
    /// compare against the same strings. `Success` displays as "success";
    /// the success value itself is not formatted.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            VMResult::Success(_) => "success",
            VMResult::Unreachable => "unreachable",
            VMResult::StackOverflow => "call stack exhausted",
            VMResult::MemoryIndexOutOfRange => "out of bounds memory access",
            VMResult::UnalignedAtomic => "unaligned atomic",
            VMResult::TableIndexOutOfRange => "out of bounds table access",
            VMResult::CallIndirectInvalidType => "indirect call type mismatch",
            VMResult::TableUninitialized => "uninitialized element",
            VMResult::Unlinkable => "unlinkable",
            VMResult::MemoryAllocationFailed => "memory allocation failed",
            VMResult::InvalidOperand => "invalid operand",
            VMResult::DivideByZero => "integer divide by zero",
            VMResult::IntegerOverflow => "integer overflow",
            VMResult::InvalidConversion => "invalid conversion to integer",
            VMResult::Unimplemented => "unimplemented",
            VMResult::FuelExhausted => "fuel exhausted",
            VMResult::Cancelled => "cancelled",
        })
    }
}

impl<V: fmt::Debug> std::error::Error for VMResult<V> {}
#[macro_export]
/// Propagates a non-success [`VMResult`](crate::VMResult) from the current function.
macro_rules! vm_try {
    ($expr: expr) => {
        match $expr {
            VMResult::Success(v) => v,
            other => return other.propagate_failure(),
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

    /// Re-types a non-success result so it can be returned from a caller
    /// whose success type differs, preserving the exact failure variant.
    ///
    /// This is the propagation step used by [`vm_try!`](crate::vm_try): the
    /// variant-by-variant conversion lives here, once, instead of being
    /// inlined at every dispatch call site, which keeps the interpreter's
    /// hot handlers small. It is marked cold because it only runs on the
    /// failure path.
    ///
    /// # Panics
    ///
    /// Panics when called on a `Success` value; there is no success value of
    /// the target type to convert into. `vm_try!` only calls this on the
    /// non-success branch.
    #[cold]
    pub fn propagate_failure<U>(self) -> VMResult<U> {
        match self {
            VMResult::Success(_) => {
                unreachable!("propagate_failure called on a Success VMResult")
            }
            VMResult::Unreachable => VMResult::Unreachable,
            VMResult::StackOverflow => VMResult::StackOverflow,
            VMResult::MemoryIndexOutOfRange => VMResult::MemoryIndexOutOfRange,
            VMResult::UnalignedAtomic => VMResult::UnalignedAtomic,
            VMResult::TableIndexOutOfRange => VMResult::TableIndexOutOfRange,
            VMResult::CallIndirectInvalidType => VMResult::CallIndirectInvalidType,
            VMResult::TableUninitialized => VMResult::TableUninitialized,
            VMResult::Unlinkable => VMResult::Unlinkable,
            VMResult::MemoryAllocationFailed => VMResult::MemoryAllocationFailed,
            VMResult::InvalidOperand => VMResult::InvalidOperand,
            VMResult::DivideByZero => VMResult::DivideByZero,
            VMResult::IntegerOverflow => VMResult::IntegerOverflow,
            VMResult::InvalidConversion => VMResult::InvalidConversion,
            VMResult::Unimplemented => VMResult::Unimplemented,
            VMResult::FuelExhausted => VMResult::FuelExhausted,
            VMResult::Cancelled => VMResult::Cancelled,
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
            VMResult::DivideByZero => {
                panic!("called `VMResult::unwrap()` on an `Err` value: DivideByZero")
            }
            VMResult::IntegerOverflow => {
                panic!("called `VMResult::unwrap()` on an `Err` value: IntegerOverflow")
            }
            VMResult::InvalidConversion => {
                panic!("called `VMResult::unwrap()` on an `Err` value: InvalidConversion")
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
