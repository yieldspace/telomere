use std::collections::VecDeque;

use crate::common::{BlockType, FuncType, ResultType, TypeIdx, ValType};

use super::instruction::BlockKind;
use super::validate::assert_valtype;
use super::{Result, WasmParserError};
#[derive(Debug)]
pub enum MaybeUnreachable {
    Unreachable(bool),
    Normal(ValType),
}
#[derive(Debug)]
pub struct TypeChecker {
    types: Vec<MaybeUnreachable>,
    blocks: VecDeque<(BlockKind, BlockType, usize)>,
    max_stack_bytes: u32,
}
impl TypeChecker {
    pub fn new(typeidx: TypeIdx) -> Self {
        let mut this = Self {
            types: vec![],
            blocks: VecDeque::from([(BlockKind::Block, BlockType::TypeIdx(typeidx), 0)]),
            max_stack_bytes: 0,
        };
        this.observe_max_stack_bytes();
        this
    }
    pub fn get_block(&self, idx: usize) -> Result<(&BlockKind, &BlockType, &usize)> {
        self.blocks
            .get(idx)
            .ok_or(WasmParserError::InvalidStackValTypeAny)
            .map(|(a, b, c)| (a, b, c))
    }
    pub fn current_block(&self) -> Result<(&BlockKind, &BlockType, &usize)> {
        self.get_block(0)
    }
    pub fn block_base(&self) -> Result<usize> {
        Ok(*self.current_block()?.2)
    }

    pub fn check_block_base(&self) -> Result<()> {
        if self.types.len() < self.block_base()? {
            Err(WasmParserError::InvalidStackValTypeAny)?
        }
        Ok(())
    }
    pub fn pop(&mut self) -> Result<MaybeUnreachable> {
        let v = self.types.pop();
        self.check_block_base()?;
        let v = v.ok_or(WasmParserError::InvalidStackValTypeAny)?;
        if let MaybeUnreachable::Unreachable(_) = v {
            self.types.push(MaybeUnreachable::Unreachable(false));
        }
        self.observe_max_stack_bytes();
        Ok(v)
    }
    pub fn push(&mut self, vt: ValType) {
        self.types.push(MaybeUnreachable::Normal(vt));
        self.observe_max_stack_bytes();
    }
    pub fn unreachable(&mut self) {
        self.types.push(MaybeUnreachable::Unreachable(false));
        self.observe_max_stack_bytes();
    }
    pub fn push_any(&mut self) {
        self.types.push(MaybeUnreachable::Unreachable(true));
        self.observe_max_stack_bytes();
    }
    pub fn check(&mut self, input: &[ValType]) -> Result<()> {
        let mut iter = self.types[self.block_base()?..].iter().rev();
        let mut current = iter.next();
        for ty in input.iter().rev() {
            if let Some(v) = current {
                match v {
                    MaybeUnreachable::Unreachable(_) => {
                        // ok
                    }
                    MaybeUnreachable::Normal(x) => {
                        assert_valtype(*ty, Some(*x))?;
                        current = iter.next();
                    }
                }
            } else {
                Err(WasmParserError::InvalidStackValTypeAny)?
            }
        }
        Ok(())
    }
    pub fn op(&mut self, input: &[ValType], output: &[ValType]) -> Result<()> {
        for input in input.iter().rev() {
            if let MaybeUnreachable::Normal(v) = self.pop()? {
                assert_valtype(*input, Some(v))?;
            }
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
        self.observe_max_stack_bytes();
    }

    pub fn leave_block(&mut self) -> Result<()> {
        let mut last_is_unreachable = true;
        for ty in self.types.drain(self.block_base()?..) {
            last_is_unreachable = matches!(ty, MaybeUnreachable::Unreachable(false));
        }
        if !last_is_unreachable {
            Err(WasmParserError::InvalidStackValTypeAny)?
        }
        self.blocks.pop_front();
        self.observe_max_stack_bytes();
        Ok(())
    }
    pub fn block_base_stack_size(&self) -> Result<u32> {
        let mut size = 0;
        for ty in &self.types[..self.block_base()?] {
            if let MaybeUnreachable::Normal(v) = ty {
                size += v.stack_size().u32()
            } else {
                unreachable!()
            }
        }
        Ok(size)
    }
    pub fn reset_stack(&mut self) -> Result<()> {
        self.types.truncate(self.block_base()?);
        self.observe_max_stack_bytes();
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

    pub fn max_stack_byte_size(&self) -> u32 {
        self.max_stack_bytes
    }

    pub fn current_stack_byte_size(&self) -> Option<u32> {
        let mut size = 0u32;
        for ty in &self.types {
            match ty {
                MaybeUnreachable::Normal(v) => size += v.stack_size().u32(),
                MaybeUnreachable::Unreachable(_) => return None,
            }
        }
        Some(size)
    }

    pub fn current_ref_offsets_from_operand_base(&self) -> Option<Vec<u32>> {
        let mut offsets = Vec::new();
        let mut cursor = 0u32;
        for ty in &self.types {
            let ty = match ty {
                MaybeUnreachable::Normal(v) => *v,
                MaybeUnreachable::Unreachable(_) => return None,
            };
            if matches!(ty, ValType::FuncRef | ValType::ExternRef) {
                offsets.push(cursor);
            }
            cursor += ty.stack_size().u32();
        }
        Some(offsets)
    }

    fn observe_max_stack_bytes(&mut self) {
        if let Some(size) = self.current_stack_byte_size() {
            self.max_stack_bytes = self.max_stack_bytes.max(size);
        }
    }
}
