use std::ops::{Deref, DerefMut};

use super::fusion;
use crate::common::{Instr, Op, Operand};

pub(crate) struct InstructionGenerator {
    instr: Vec<Instr>,
    pending: Vec<Instr>,
    unreachable: Vec<bool>,
    current_instruction_fusible: Vec<bool>,
    fusion_enabled: Vec<bool>,
}
impl InstructionGenerator {
    pub(crate) fn new() -> Self {
        Self {
            instr: vec![],
            pending: vec![],
            unreachable: vec![false],
            current_instruction_fusible: vec![],
            fusion_enabled: vec![true],
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
    pub(crate) fn begin_instruction(&mut self) {
        self.current_instruction_fusible.push(false);
    }
    pub(crate) fn set_current_instruction_fusible(&mut self) {
        if self.fusion_enabled() {
            if let Some(flag) = self.current_instruction_fusible.last_mut() {
                *flag = true;
            }
        }
    }
    pub(crate) fn finish_instruction(&mut self) {
        let fusible = self.current_instruction_fusible.pop().unwrap_or(false);
        if fusible && self.pending.len() >= 64 {
            self.flush_pending();
        }
    }
    pub(crate) fn enable_fusion(&mut self) {
        *self.fusion_enabled.last_mut().unwrap() = true;
    }
    fn fusion_enabled(&self) -> bool {
        self.fusion_enabled.last().copied().unwrap_or(false)
    }
    fn current_instruction_fusible(&self) -> bool {
        self.current_instruction_fusible
            .last()
            .copied()
            .unwrap_or(false)
    }
    pub(crate) fn push(&mut self, instr: Instr) -> &mut Self {
        if !self.unreachable.last().unwrap() {
            if self.current_instruction_fusible() {
                self.pending.push(instr);
            } else {
                self.flush_pending();
                self.instr.push(instr);
            }
        }
        self
    }
    pub(crate) fn force_push(&mut self, instr: Instr) -> &mut Self {
        self.flush_pending();
        self.instr.push(instr);
        self
    }
    pub(crate) fn flush_pending(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        fusion::emit_fused_region(&self.pending, &mut self.instr);
        self.pending.clear();
    }
    pub(crate) fn len(&mut self) -> usize {
        self.flush_pending();
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
        self.flush_pending();
        let unreachable = self.is_unreachable();
        self.unreachable.push(unreachable);
        self.fusion_enabled.push(false);
    }
    pub(crate) fn leave_block(&mut self) {
        self.flush_pending();
        self.unreachable.pop();
        self.fusion_enabled.pop();
    }
    pub(crate) fn build(mut self) -> Vec<Instr> {
        self.flush_pending();
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
