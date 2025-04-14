use std::collections::VecDeque;

use crate::common::Instr;
pub enum JumpResolverDSL {
    EnterForwardJumpBlock,
    EnterBackwardJumpBlock(u32),
    Br(u32, u32),
    Return(u32),
    LeaveBlock(u32),
}
enum JumpResolverState {
    Resolved(u32),
    Lazy(Vec<u32>),
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
        let mut block: VecDeque<JumpResolverState> = VecDeque::new();
        for dsl in &self.inner {
            match dsl {
                JumpResolverDSL::EnterForwardJumpBlock => {
                    block.push_front(JumpResolverState::Lazy(vec![]));
                }
                JumpResolverDSL::EnterBackwardJumpBlock(addr) => {
                    block.push_front(JumpResolverState::Resolved(*addr));
                }
                JumpResolverDSL::Br(id, program_addr) => match &mut block[*id as usize] {
                    JumpResolverState::Resolved(jump_addr) => {
                        program[*program_addr as usize].operand.jump_addr = *jump_addr;
                    }
                    JumpResolverState::Lazy(items) => {
                        items.push(*program_addr);
                    }
                },
                JumpResolverDSL::Return(program_addr) => match block.back_mut().unwrap() {
                    JumpResolverState::Resolved(jump_addr) => {
                        program[*program_addr as usize].operand.jump_addr = *jump_addr;
                    }
                    JumpResolverState::Lazy(items) => {
                        items.push(*program_addr);
                    }
                },
                JumpResolverDSL::LeaveBlock(program_addr) => match block.pop_front().unwrap() {
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
