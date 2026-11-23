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

/// Bit layout of a TRAP exit's `value` (#209): the low 8 bits carry the
/// `vm_result_code` (codes fit easily; the largest is 17) and the
/// remaining high bits carry a 1-based trap-site id into the trapping
/// function's trap-site table. Site 0 means "no site": exits raised by
/// Rust stubs keep the historic bare-code encoding, so a site-less trap
/// exit is indistinguishable from a pre-#209 one.
pub(crate) const TRAP_CODE_MASK: u64 = 0xff;
pub(crate) const TRAP_SITE_SHIFT: u32 = 8;

/// Packs a trap code and a 1-based trap-site id into a TRAP exit value.
pub(crate) const fn pack_trap_value(code: u64, site: u64) -> u64 {
    debug_assert!(code <= TRAP_CODE_MASK);
    (site << TRAP_SITE_SHIFT) | (code & TRAP_CODE_MASK)
}

/// Splits a TRAP exit value into its trap code and its 1-based trap-site
/// id (0 when the exit carries no site).
pub(crate) const fn split_trap_value(value: u64) -> (u64, u64) {
    (value & TRAP_CODE_MASK, value >> TRAP_SITE_SHIFT)
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
        VMResult::DivideByZero => 14,
        VMResult::IntegerOverflow => 15,
        VMResult::InvalidConversion => 16,
        VMResult::ResourceLimitExceeded => 17,
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
        14 => VMResult::DivideByZero,
        15 => VMResult::IntegerOverflow,
        16 => VMResult::InvalidConversion,
        17 => VMResult::ResourceLimitExceeded,
        _ => VMResult::InvalidOperand,
    }
}

#[cfg(test)]
mod tests {
    use super::{pack_trap_value, split_trap_value, vm_result_code, vm_result_from_code};
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

    #[test]
    fn trap_site_packing_round_trips_and_site_zero_is_the_bare_code() {
        // Site 0 keeps the historic encoding exactly.
        assert_eq!(pack_trap_value(14, 0), 14);
        assert_eq!(split_trap_value(14), (14, 0));
        for code in [1_u64, 3, 14, 16] {
            for site in [1_u64, 2, 255, 4096, 1 << 40] {
                assert_eq!(split_trap_value(pack_trap_value(code, site)), (code, site));
            }
        }
    }

    #[test]
    fn precise_trap_causes_round_trip_through_jit_trap_codes() {
        assert_eq!(vm_result_code(VMResult::<()>::DivideByZero), 14);
        assert_eq!(vm_result_code(VMResult::<()>::IntegerOverflow), 15);
        assert_eq!(vm_result_code(VMResult::<()>::InvalidConversion), 16);
        assert!(matches!(vm_result_from_code(14), VMResult::DivideByZero));
        assert!(matches!(vm_result_from_code(15), VMResult::IntegerOverflow));
        assert!(matches!(
            vm_result_from_code(16),
            VMResult::InvalidConversion
        ));
    }

    #[test]
    fn resource_limit_exceeded_round_trips_through_jit_trap_code() {
        assert_eq!(vm_result_code(VMResult::<()>::ResourceLimitExceeded), 17);
        assert!(matches!(
            vm_result_from_code(17),
            VMResult::ResourceLimitExceeded
        ));
    }
}
