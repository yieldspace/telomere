use crate::runtime::core::stack::CallStack;

mod handler;
mod stack;

pub struct ExecuteContext {
    pub stack: CallStack
}

pub type InstructionHandler = fn(*const CoreInstruction, context: &mut ExecuteContext);

pub struct CoreInstruction {
    pub handler: InstructionHandler
}

pub enum InstructionData {

}
