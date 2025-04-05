use std::collections::VecDeque;

use crate::common::{BlockType, FuncType, ResultType, TypeIdx, ValType};

use super::instruction::BlockKind;
use super::validate::assert_valtype;
use super::{Result, WasmParserError};

#[derive(Debug)]
pub struct TypeChecker {
    types: Vec<ValType>,
    blocks: VecDeque<(BlockKind, BlockType, usize)>,
}
impl TypeChecker {
    pub fn new(typeidx: TypeIdx) -> Self {
        Self {
            types: vec![],
            blocks: VecDeque::from([(BlockKind::Block, BlockType::TypeIdx(typeidx), 0)]),
        }
    }
    pub fn get_block(&self, idx: usize) -> Result<&(BlockKind, BlockType, usize)> {
        self.blocks
            .get(idx)
            .ok_or_else(|| WasmParserError::InvalidStackValTypeAny)
    }
    pub fn current_block(&self) -> Result<&(BlockKind, BlockType, usize)> {
        self.blocks
            .front()
            .ok_or_else(|| WasmParserError::InvalidStackValTypeAny)
    }
    pub fn block_base(&self) -> Result<usize> {
        Ok(self.current_block()?.2)
    }

    pub fn check_block_base(&self) -> Result<()> {
        if self.types.len() < self.block_base()? {
            Err(WasmParserError::InvalidStackValTypeAny)?
        }
        Ok(())
    }
    pub fn pop(&mut self) -> Result<ValType> {
        let v = self.types.pop();
        self.check_block_base()?;
        v.ok_or_else(|| WasmParserError::InvalidStackValTypeAny)
    }
    pub fn push(&mut self, vt: ValType) {
        self.types.push(vt);
    }
    pub fn op(&mut self, input: &[ValType], output: &[ValType]) -> Result<()> {
        for input in input.iter().rev() {
            assert_valtype(*input, Some(self.pop()?))?;
        }
        for output in output.iter() {
            self.push(*output);
        }
        Ok(())
    }
    pub fn op_result_type(&mut self, input: &ResultType, output: &ResultType) -> Result<()> {
        self.op(&input.0, &output.0)
    }
    pub fn op_func_type(&mut self, ft: &FuncType) -> Result<()> {
        self.op_result_type(&ft.0, &ft.1)
    }
    pub fn enter_block(&mut self, kind: BlockKind, block_type: BlockType) {
        self.blocks.push_front((kind, block_type, self.types.len()));
    }
    pub fn leave_block(&mut self) -> Result<()> {
        if self.block_base()? != self.types.len() {
            Err(WasmParserError::InvalidStackValTypeAny)?
        }
        self.blocks.pop_front();
        Ok(())
    }

    pub fn block_base_stack_size(&self) -> Result<u32> {
        Ok(self.types[..self.block_base()?]
            .iter()
            .map(|v| v.stack_size().u32())
            .sum())
    }
    pub fn reset_stack(&mut self) -> Result<()> {
        self.types.truncate(self.block_base()?);
        Ok(())
    }
    pub fn load_op(&mut self, ty: ValType) -> Result<()> {
        self.op(&[ValType::I32], &[ty])
    }
    pub fn store_op(&mut self, ty: ValType) -> Result<()> {
        self.op(&[ValType::I32, ty], &[])
    }
    pub fn cond_op(&mut self, ty: ValType) -> Result<()> {
        self.op(&[ty, ty], &[ValType::I32])
    }
    pub fn unary_op(&mut self, ty: ValType) -> Result<()> {
        self.op(&[ty, ty], &[ty])
    }
    pub fn binary_op(&mut self, ty: ValType) -> Result<()> {
        self.op(&[ty], &[ty])
    }
}
