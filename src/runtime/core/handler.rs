use crate::runtime::core::{CoreInstruction, ExecuteContext};


pub unsafe fn load(instruction: *const CoreInstruction, context: &mut ExecuteContext) {
    ((*instruction).handler)(instruction.offset(1), context);
}
