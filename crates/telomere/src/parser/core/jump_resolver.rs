use crate::common::Instr;
use smallvec::SmallVec;

pub enum JumpResolverDSL {
    EnterForwardJumpBlock,
    EnterBackwardJumpBlock(u32),
    Br(u32, u32),
    Return(u32),
    LeaveBlock(u32),
}
enum JumpResolverState {
    Resolved(u32),
    Lazy(SmallVec<[u32; 4]>),
}
pub struct JumpResolver {
    inner: Vec<JumpResolverDSL>,
}

impl JumpResolver {
    pub fn new() -> Self {
        Self { inner: Vec::new() }
    }
    pub fn push(&mut self, dsl: JumpResolverDSL) {
        self.inner.push(dsl);
    }
    pub fn evaluate(&self, program: &mut [Instr]) {
        let mut block: Vec<JumpResolverState> = Vec::new();
        for dsl in &self.inner {
            match dsl {
                JumpResolverDSL::EnterForwardJumpBlock => {
                    block.push(JumpResolverState::Lazy(SmallVec::new()));
                }
                JumpResolverDSL::EnterBackwardJumpBlock(addr) => {
                    block.push(JumpResolverState::Resolved(*addr));
                }
                JumpResolverDSL::Br(id, program_addr) => {
                    let block_idx = block
                        .len()
                        .checked_sub(1 + *id as usize)
                        .expect("validated jump target block must exist");
                    match &mut block[block_idx] {
                        JumpResolverState::Resolved(jump_addr) => {
                            program[*program_addr as usize].operand.jump_addr = *jump_addr;
                        }
                        JumpResolverState::Lazy(items) => {
                            items.push(*program_addr);
                        }
                    }
                }
                JumpResolverDSL::Return(program_addr) => match block.first_mut().unwrap() {
                    JumpResolverState::Resolved(jump_addr) => {
                        program[*program_addr as usize].operand.jump_addr = *jump_addr;
                    }
                    JumpResolverState::Lazy(items) => {
                        items.push(*program_addr);
                    }
                },
                JumpResolverDSL::LeaveBlock(program_addr) => match block.pop().unwrap() {
                    JumpResolverState::Resolved(_jump_addr) => {} // ok
                    JumpResolverState::Lazy(items) => {
                        for idx in items {
                            program[idx as usize].operand.jump_addr = *program_addr;
                        }
                    }
                },
            }
        }
    }
}
