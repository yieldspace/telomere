use crate::runtime::vm::ExecuteContext;
use crate::runtime::vm::Instr;
use crate::VMResult;

macro_rules! generate_trap_func {
    ($name: ident,$instrs_name: ident,$expr: expr) => {
        #[allow(dead_code)]
        #[doc = concat!("Internal trap entrypoint `", stringify!($name), "`.\n")]
        #[doc = "\n"]
        #[doc = "Spec: [WebAssembly Core Spec](https://webassembly.github.io/spec/core/index.html)\n"]
        #[doc = "\n"]
        #[doc = "# Safety\n"]
        #[doc = "`_tail_code` and `_ctx` are ignored; this function is only reached through the generated trap jump table.\n"]
        pub(crate) unsafe fn $name(
            _tail_code: *const Instr,
            _ctx: &mut ExecuteContext,
        ) -> VMResult<()> {
            $expr
        }
        #[allow(dead_code)]
        pub(crate) const $instrs_name: [Instr; 1] = [Instr { op: $name }];
    };
}
generate_trap_func!(
    traps_call_indirect_invalid_type,
    TRAPS_CALL_INDIRECT_INVALID_TYPE,
    VMResult::CallIndirectInvalidType
);
generate_trap_func!(
    traps_invalid_operand,
    TRAPS_INVALID_OPERAND,
    VMResult::InvalidOperand
);
generate_trap_func!(
    traps_memory_index_out_of_range,
    TRAPS_MEMORY_INDEX_OUT_OF_RANGE,
    VMResult::MemoryIndexOutOfRange
);
generate_trap_func!(
    traps_unaligned_atomic,
    TRAPS_UNALIGNED_ATOMIC,
    VMResult::UnalignedAtomic
);
generate_trap_func!(
    traps_stack_overflow,
    TRAPS_STACK_OVERFLOW,
    VMResult::StackOverflow
);
generate_trap_func!(
    traps_table_index_out_of_range,
    TRAPS_TABLE_INDEX_OUT_OF_RANGE,
    VMResult::TableIndexOutOfRange
);
generate_trap_func!(
    traps_table_uninitialized,
    TRAPS_TABLE_UNINITIALIZED,
    VMResult::TableUninitialized
);
generate_trap_func!(traps_unlinkable, TRAPS_UNLINKABLE, VMResult::Unlinkable);
generate_trap_func!(
    traps_unimplemented,
    TRAPS_UNIMPLEMENTED,
    VMResult::Unimplemented
);
generate_trap_func!(traps_unreachable, TRAPS_UNREACHABLE, VMResult::Unreachable);

#[allow(dead_code)]
#[doc = "Internal helper that maps a `VMResult` trap to its trap instruction.\n"]
#[doc = "\n"]
#[doc = "Spec: [WebAssembly Core Spec](https://webassembly.github.io/spec/core/index.html)\n"]
#[doc = "\n"]
#[doc = "# Safety\n"]
#[doc = "`res` must be a trap variant; passing `VMResult::Success` is unreachable by construction.\n"]
pub(crate) unsafe fn trap_func<T>(res: VMResult<T>) -> *const Instr {
    match res {
        VMResult::CallIndirectInvalidType => TRAPS_CALL_INDIRECT_INVALID_TYPE.as_ptr(),
        VMResult::InvalidOperand => TRAPS_INVALID_OPERAND.as_ptr(),
        VMResult::MemoryIndexOutOfRange => TRAPS_MEMORY_INDEX_OUT_OF_RANGE.as_ptr(),
        VMResult::UnalignedAtomic => TRAPS_UNALIGNED_ATOMIC.as_ptr(),
        VMResult::StackOverflow => TRAPS_STACK_OVERFLOW.as_ptr(),
        VMResult::TableIndexOutOfRange => TRAPS_TABLE_INDEX_OUT_OF_RANGE.as_ptr(),
        VMResult::TableUninitialized => TRAPS_TABLE_UNINITIALIZED.as_ptr(),
        VMResult::Unlinkable => TRAPS_UNLINKABLE.as_ptr(),
        VMResult::Unimplemented => TRAPS_UNIMPLEMENTED.as_ptr(),
        VMResult::Unreachable => TRAPS_UNREACHABLE.as_ptr(),
        VMResult::Success(_) => unreachable!(),
    }
}
#[macro_export]
macro_rules! trap_func {
    ($data: expr) => {
        match $data {
            VMResult::Success(v) => v,
            other => return $crate::runtime::vm::traps::trap_func(other),
        }
    };
}
