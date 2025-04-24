use crate::binary::BinaryReader;

use super::{instruction_generator::InstructionGenerator, type_checker::TypeChecker};

pub(crate) struct SimdParserContext<'a, R: BinaryReader> {
    pub(crate) instrs: &'a mut InstructionGenerator,
    pub(crate) checker: &'a mut TypeChecker,
    pub(crate) reader: &'a mut R,
}
mod prelude {
    pub(crate) use super::SimdParserContext;
    pub(crate) use crate::common::ValType;
    pub(crate) use crate::{
        binary::BinaryReader, common::Operand, parser::core::values, runtime::vm, WasmParserError,
    };
}
pub(crate) mod v128_load {
    use super::prelude::*;

    pub(crate) const CODE: u32 = 0;
    pub(crate) fn parse<R: BinaryReader>(
        ctx: &mut SimdParserContext<R>,
    ) -> Result<usize, WasmParserError> {
        let (len, memarg) = values::parse_memarg(ctx.reader, 4)?; // TODO:
        ctx.checker.load_op(ValType::V128)?;
        ctx.instrs
            .push_with_operand(vm::simd::op_v128_load, &[Operand { memarg }]);
        Ok(len)
    }
}
