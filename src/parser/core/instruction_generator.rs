use std::ops::{Deref, DerefMut};

use crate::common::{Instr, Op, Operand};

pub(crate) struct InstructionGenerator {
    instr: Vec<Instr>,
    unreachable: Vec<bool>,
}
impl InstructionGenerator {
    pub(crate) fn new() -> Self {
        Self {
            instr: vec![],
            unreachable: vec![false],
        }
    }
    #[allow(dead_code)]
    pub(crate) fn push_instr1(&mut self, opcode: Op) -> &mut Self {
        self.push_with_operand(opcode, &[]);
        self
    }
    #[allow(dead_code)]
    pub(crate) fn push_with_operand(&mut self, opcode: Op, operands: &[Operand]) -> &mut Self {
        self.push(Instr { op: opcode });
        for operand in operands {
            self.push(Instr { operand: *operand });
        }
        self
    }
    pub(crate) fn push(&mut self, instr: Instr) -> &mut Self {
        if !self.unreachable.last().unwrap() {
            self.instr.push(instr);
        }
        self
    }
    pub(crate) fn force_push(&mut self, instr: Instr) -> &mut Self {
        self.instr.push(instr);

        self
    }
    pub(crate) fn len(&self) -> usize {
        self.instr.len()
    }
    pub(crate) fn set_unreachable(&mut self) -> &mut Self {
        *self.unreachable.last_mut().unwrap() = true;
        self
    }
    pub(crate) fn is_unreachable(&mut self) -> bool {
        *self.unreachable.last().unwrap()
    }
    pub(crate) fn enter_block(&mut self) {
        let unreachable = self.is_unreachable();
        self.unreachable.push(unreachable);
    }
    pub(crate) fn leave_block(&mut self) {
        self.unreachable.pop();
    }
    pub(crate) fn build(self) -> Vec<Instr> {
        self.instr
    }
}
impl Deref for InstructionGenerator {
    type Target = [Instr];

    fn deref(&self) -> &Self::Target {
        &self.instr[..]
    }
}
impl DerefMut for InstructionGenerator {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.instr
    }
}
