use crate::common::{ExecuteContext, Instr, VMResult};

pub(crate) type JitEntry =
    unsafe extern "C" fn(*mut ExecuteContext<'_>, *const Instr, *mut u8) -> JitNativeExit;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JitNativeExit {
    pub kind: u64,
    pub value: u64,
}

impl JitNativeExit {
    pub const KEEP_GOING: u64 = 0;
    pub const FALLBACK_INDEX: u64 = 1;
    pub const CONTINUE_PTR: u64 = 2;
    pub const TRAP: u64 = 3;
    pub const DONE: u64 = 4;
    pub const FALLBACK_PTR: u64 = 5;
    pub const PENDING: u64 = 6;

    pub const fn keep_going() -> Self {
        Self {
            kind: Self::KEEP_GOING,
            value: 0,
        }
    }

    // #202 retains the codegen ABI constructor although Rust dispatch does not call it directly.
    #[allow(dead_code)]
    pub const fn fallback_index(index: usize) -> Self {
        Self {
            kind: Self::FALLBACK_INDEX,
            value: index as u64,
        }
    }

    pub fn fallback_pc(pc: *const Instr) -> Self {
        Self {
            kind: Self::FALLBACK_PTR,
            value: pc as u64,
        }
    }

    pub fn continue_ptr(ptr: *const Instr) -> Self {
        Self {
            kind: Self::CONTINUE_PTR,
            value: ptr as u64,
        }
    }

    pub const fn done() -> Self {
        Self {
            kind: Self::DONE,
            value: 0,
        }
    }

    pub const fn done_with_value(value: u64) -> Self {
        Self {
            kind: Self::DONE,
            value,
        }
    }

    pub const fn pending() -> Self {
        Self {
            kind: Self::PENDING,
            value: 0,
        }
    }

    pub fn trap<T>(result: VMResult<T>) -> Self {
        Self {
            kind: Self::TRAP,
            value: vm_result_code(result),
        }
    }
}

pub(crate) fn vm_result_code<T>(result: VMResult<T>) -> u64 {
    match result {
        VMResult::Success(_) => 0,
        VMResult::Unreachable => 1,
        VMResult::StackOverflow => 2,
        VMResult::MemoryIndexOutOfRange => 3,
        VMResult::TableIndexOutOfRange => 4,
        VMResult::CallIndirectInvalidType => 5,
        VMResult::TableUninitialized => 6,
        VMResult::Unlinkable => 7,
        VMResult::MemoryAllocationFailed => 11,
        VMResult::InvalidOperand => 8,
        VMResult::UnalignedAtomic => 9,
        VMResult::Unimplemented => 10,
        VMResult::FuelExhausted => 12,
        VMResult::Cancelled => 13,
    }
}

pub(crate) fn vm_result_from_code(code: u64) -> VMResult<()> {
    match code {
        0 => {
            debug_assert_ne!(code, 0, "JIT trap exit cannot carry Success code 0");
            VMResult::InvalidOperand
        }
        1 => VMResult::Unreachable,
        2 => VMResult::StackOverflow,
        3 => VMResult::MemoryIndexOutOfRange,
        4 => VMResult::TableIndexOutOfRange,
        5 => VMResult::CallIndirectInvalidType,
        6 => VMResult::TableUninitialized,
        7 => VMResult::Unlinkable,
        11 => VMResult::MemoryAllocationFailed,
        8 => VMResult::InvalidOperand,
        9 => VMResult::UnalignedAtomic,
        10 => VMResult::Unimplemented,
        12 => VMResult::FuelExhausted,
        13 => VMResult::Cancelled,
        _ => VMResult::InvalidOperand,
    }
}

#[cfg(test)]
mod tests {
    use super::{vm_result_code, vm_result_from_code};
    use crate::common::VMResult;

    #[test]
    fn memory_allocation_failed_round_trips_through_jit_trap_code() {
        assert_eq!(vm_result_code(VMResult::<()>::MemoryAllocationFailed), 11);
        assert!(matches!(
            vm_result_from_code(11),
            VMResult::MemoryAllocationFailed
        ));
    }

    #[test]
    fn interruptions_round_trip_through_jit_trap_codes() {
        assert_eq!(vm_result_code(VMResult::<()>::FuelExhausted), 12);
        assert_eq!(vm_result_code(VMResult::<()>::Cancelled), 13);
        assert!(matches!(vm_result_from_code(12), VMResult::FuelExhausted));
        assert!(matches!(vm_result_from_code(13), VMResult::Cancelled));
    }
}
