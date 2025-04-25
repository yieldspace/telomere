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
pub(crate) mod i8x16_extract_lane_s {
    use super::prelude::*;
    pub(crate) const CODE: u32 = 21;
    pub(crate) fn parse<R: BinaryReader>(
        ctx: &mut SimdParserContext<R>,
    ) -> Result<usize, WasmParserError> {
        let (len, lane) = values::parse_byte(ctx.reader)?;
        ctx.checker.op(&[ValType::V128], &[ValType::I32])?;

        ctx.instrs.push_with_operand(
            vm::simd::op_i8x16_extract_lane_s,
            &[Operand { u32: lane as u32 }],
        );
        Ok(len)
    }
}
pub(crate) mod i8x16_eq {
    use super::prelude::*;
    pub(crate) const CODE: u32 = 35;
    pub(crate) fn parse<R: BinaryReader>(
        ctx: &mut SimdParserContext<R>,
    ) -> Result<usize, WasmParserError> {
        ctx.checker.unary_op(ValType::V128)?;
        ctx.instrs.push_instr1(vm::simd::op_i8x16_eq);
        Ok(0)
    }
}
pub(crate) mod v128_not {
    use super::prelude::*;
    pub(crate) const CODE: u32 = 77;
    pub(crate) fn parse<R: BinaryReader>(
        ctx: &mut SimdParserContext<R>,
    ) -> Result<usize, WasmParserError> {
        ctx.checker.binary_op(ValType::V128)?;
        ctx.instrs.push_instr1(vm::simd::op_v128_not);
        Ok(0)
    }
}
pub(crate) mod i8x16_all_true {
    use super::prelude::*;
    pub(crate) const CODE: u32 = 99;
    pub(crate) fn parse<R: BinaryReader>(
        ctx: &mut SimdParserContext<R>,
    ) -> Result<usize, WasmParserError> {
        ctx.checker.op(&[ValType::V128], &[ValType::I32])?;
        ctx.instrs.push_instr1(vm::simd::op_i8x16_all_true);
        Ok(0)
    }
}
pub(crate) mod v128_bitselect {
    use super::prelude::*;
    pub(crate) const CODE: u32 = 82;
    pub(crate) fn parse<R: BinaryReader>(
        ctx: &mut SimdParserContext<R>,
    ) -> Result<usize, WasmParserError> {
        ctx.checker.op(
            &[ValType::V128, ValType::V128, ValType::V128],
            &[ValType::V128],
        )?;
        ctx.instrs.push_instr1(vm::simd::op_v128_bitselect);
        Ok(0)
    }
}
pub(crate) mod i8x16_shl {
    use super::prelude::*;
    pub(crate) const CODE: u32 = 107;
    pub(crate) fn parse<R: BinaryReader>(
        ctx: &mut SimdParserContext<R>,
    ) -> Result<usize, WasmParserError> {
        ctx.checker
            .op(&[ValType::V128, ValType::I32], &[ValType::V128])?;
        ctx.instrs.push_instr1(vm::simd::op_i8x16_shl);
        Ok(0)
    }
}
pub(crate) mod i8x16_add {
    use super::prelude::*;
    pub(crate) const CODE: u32 = 110;
    pub(crate) fn parse<R: BinaryReader>(
        ctx: &mut SimdParserContext<R>,
    ) -> Result<usize, WasmParserError> {
        ctx.checker.unary_op(ValType::V128)?;
        ctx.instrs.push_instr1(vm::simd::op_i8x16_add);
        Ok(0)
    }
}
pub(crate) mod i8x16_sub {
    use super::prelude::*;
    pub(crate) const CODE: u32 = 113;
    pub(crate) fn parse<R: BinaryReader>(
        ctx: &mut SimdParserContext<R>,
    ) -> Result<usize, WasmParserError> {
        ctx.checker.unary_op(ValType::V128)?;
        ctx.instrs.push_instr1(vm::simd::op_i8x16_sub);
        Ok(0)
    }
}
pub(crate) mod f32x4_mul {
    use super::prelude::*;
    pub(crate) const CODE: u32 = 230;
    pub(crate) fn parse<R: BinaryReader>(
        ctx: &mut SimdParserContext<R>,
    ) -> Result<usize, WasmParserError> {
        ctx.checker.unary_op(ValType::V128)?;
        ctx.instrs.push_instr1(vm::simd::op_f32x4_mul);
        Ok(0)
    }
}