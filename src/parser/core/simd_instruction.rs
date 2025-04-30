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

macro_rules! unary_op_simd_parser {
    ($name: ident,$code: expr) => {
        pub(crate) mod $name {
            use super::prelude::*;
            pub(crate) const CODE: u32 = $code;
            pub(crate) fn parse<R: BinaryReader>(
                ctx: &mut SimdParserContext<R>,
            ) -> Result<usize, WasmParserError> {
                ctx.checker.unary_op(ValType::V128)?;
                ctx.instrs.push_instr1(vm::simd::$name);
                Ok(0)
            }
        }
    };
}
macro_rules! binary_op_simd_parser {
    ($name: ident,$code: expr) => {
        pub(crate) mod $name {
            use super::prelude::*;
            pub(crate) const CODE: u32 = $code;
            pub(crate) fn parse<R: BinaryReader>(
                ctx: &mut SimdParserContext<R>,
            ) -> Result<usize, WasmParserError> {
                ctx.checker.binary_op(ValType::V128)?;
                ctx.instrs.push_instr1(vm::simd::$name);
                Ok(0)
            }
        }
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

pub(crate) mod v128_load8x8_s {
    use super::prelude::*;
    pub(crate) const CODE: u32 = 1;
    pub(crate) fn parse<R: BinaryReader>(
        ctx: &mut SimdParserContext<R>,
    ) -> Result<usize, WasmParserError> {
        let (len, memarg) = values::parse_memarg(ctx.reader, 8)?;
        ctx.checker.load_op(ValType::V128)?;
        ctx.instrs
            .push_with_operand(vm::simd::v128_load8x8_s, &[Operand { memarg }]);
        Ok(len)
    }
}

pub(crate) mod v128_load8x8_u {
    use super::prelude::*;
    pub(crate) const CODE: u32 = 2;
    pub(crate) fn parse<R: BinaryReader>(
        ctx: &mut SimdParserContext<R>,
    ) -> Result<usize, WasmParserError> {
        let (len, memarg) = values::parse_memarg(ctx.reader, 8)?;
        ctx.checker.load_op(ValType::V128)?;
        ctx.instrs
            .push_with_operand(vm::simd::v128_load8x8_u, &[Operand { memarg }]);
        Ok(len)
    }
}

pub(crate) mod v128_load16x4_s {
    use super::prelude::*;
    pub(crate) const CODE: u32 = 3;
    pub(crate) fn parse<R: BinaryReader>(
        ctx: &mut SimdParserContext<R>,
    ) -> Result<usize, WasmParserError> {
        let (len, memarg) = values::parse_memarg(ctx.reader, 8)?;
        ctx.checker.load_op(ValType::V128)?;
        ctx.instrs
            .push_with_operand(vm::simd::v128_load16x4_s, &[Operand { memarg }]);
        Ok(len)
    }
}

pub(crate) mod v128_load16x4_u {
    use super::prelude::*;
    pub(crate) const CODE: u32 = 4;
    pub(crate) fn parse<R: BinaryReader>(
        ctx: &mut SimdParserContext<R>,
    ) -> Result<usize, WasmParserError> {
        let (len, memarg) = values::parse_memarg(ctx.reader, 8)?;
        ctx.checker.load_op(ValType::V128)?;
        ctx.instrs
            .push_with_operand(vm::simd::v128_load16x4_u, &[Operand { memarg }]);
        Ok(len)
    }
}

pub(crate) mod v128_load32x2_s {
    use super::prelude::*;
    pub(crate) const CODE: u32 = 5;
    pub(crate) fn parse<R: BinaryReader>(
        ctx: &mut SimdParserContext<R>,
    ) -> Result<usize, WasmParserError> {
        let (len, memarg) = values::parse_memarg(ctx.reader, 8)?;
        ctx.checker.load_op(ValType::V128)?;
        ctx.instrs
            .push_with_operand(vm::simd::v128_load32x2_s, &[Operand { memarg }]);
        Ok(len)
    }
}

pub(crate) mod v128_load32x2_u {
    use super::prelude::*;
    pub(crate) const CODE: u32 = 6;
    pub(crate) fn parse<R: BinaryReader>(
        ctx: &mut SimdParserContext<R>,
    ) -> Result<usize, WasmParserError> {
        let (len, memarg) = values::parse_memarg(ctx.reader, 8)?;
        ctx.checker.load_op(ValType::V128)?;
        ctx.instrs
            .push_with_operand(vm::simd::v128_load32x2_u, &[Operand { memarg }]);
        Ok(len)
    }
}

pub(crate) mod v128_load8_splat {
    use super::prelude::*;
    pub(crate) const CODE: u32 = 7;
    pub(crate) fn parse<R: BinaryReader>(
        ctx: &mut SimdParserContext<R>,
    ) -> Result<usize, WasmParserError> {
        let (len, memarg) = values::parse_memarg(ctx.reader, 8)?;
        ctx.checker.load_op(ValType::V128)?;
        ctx.instrs
            .push_with_operand(vm::simd::v128_load8_splat, &[Operand { memarg }]);
        Ok(len)
    }
}

pub(crate) mod v128_load16_splat {
    use super::prelude::*;
    pub(crate) const CODE: u32 = 8;
    pub(crate) fn parse<R: BinaryReader>(
        ctx: &mut SimdParserContext<R>,
    ) -> Result<usize, WasmParserError> {
        let (len, memarg) = values::parse_memarg(ctx.reader, 8)?;
        ctx.checker.load_op(ValType::V128)?;
        ctx.instrs
            .push_with_operand(vm::simd::v128_load16_splat, &[Operand { memarg }]);
        Ok(len)
    }
}
pub(crate) mod v128_load32_splat {
    use super::prelude::*;
    pub(crate) const CODE: u32 = 9;
    pub(crate) fn parse<R: BinaryReader>(
        ctx: &mut SimdParserContext<R>,
    ) -> Result<usize, WasmParserError> {
        let (len, memarg) = values::parse_memarg(ctx.reader, 8)?;
        ctx.checker.load_op(ValType::V128)?;
        ctx.instrs
            .push_with_operand(vm::simd::v128_load32_splat, &[Operand { memarg }]);
        Ok(len)
    }
}
pub(crate) mod v128_load64_splat {
    use super::prelude::*;
    pub(crate) const CODE: u32 = 10;
    pub(crate) fn parse<R: BinaryReader>(
        ctx: &mut SimdParserContext<R>,
    ) -> Result<usize, WasmParserError> {
        let (len, memarg) = values::parse_memarg(ctx.reader, 8)?;
        ctx.checker.load_op(ValType::V128)?;
        ctx.instrs
            .push_with_operand(vm::simd::v128_load64_splat, &[Operand { memarg }]);
        Ok(len)
    }
}

pub(crate) mod v128_store {
    use super::prelude::*;

    pub(crate) const CODE: u32 = 11;
    pub(crate) fn parse<R: BinaryReader>(
        ctx: &mut SimdParserContext<R>,
    ) -> Result<usize, WasmParserError> {
        let (len, memarg) = values::parse_memarg(ctx.reader, 4)?; // TODO:
        ctx.checker.store_op(ValType::V128)?;
        ctx.instrs
            .push_with_operand(vm::simd::v128_store, &[Operand { memarg }]);
        Ok(len)
    }
}

pub(crate) mod v128_const {
    use super::prelude::*;

    pub(crate) const CODE: u32 = 12;
    pub(crate) fn parse<R: BinaryReader>(
        ctx: &mut SimdParserContext<R>,
    ) -> Result<usize, WasmParserError> {
        let src = ctx.reader.read_exact::<16>()?;
        let mut right_buf = [0u8; 8];
        let mut left_buf = [0u8; 8];
        left_buf.copy_from_slice(&src[0..8]);
        right_buf.copy_from_slice(&src[8..16]);
        ctx.checker.op(&[], &[ValType::V128])?;
        ctx.instrs.push_with_operand(
            vm::simd::v128_const,
            &[
                Operand { encoded: left_buf },
                Operand { encoded: right_buf },
            ],
        );
        Ok(16)
    }
}

unary_op_simd_parser!(i8x16_swizzle, 14);

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

macro_rules! shift_instruction_parser {
    ($name: ident,$code: expr) => {
        pub(crate) mod $name {
            use super::prelude::*;
            pub(crate) const CODE: u32 = $code;
            pub(crate) fn parse<R: BinaryReader>(
                ctx: &mut SimdParserContext<R>,
            ) -> Result<usize, WasmParserError> {
                ctx.checker
                    .op(&[ValType::V128, ValType::I32], &[ValType::V128])?;
                ctx.instrs.push_instr1(vm::simd::$name);
                Ok(0)
            }
        }        
    };
}

shift_instruction_parser!(i8x16_shl,107);
shift_instruction_parser!(i8x16_shr,108);
shift_instruction_parser!(u8x16_shr,109);
unary_op_simd_parser!(i8x16_add, 110);
unary_op_simd_parser!(i8x16_sub, 113);
unary_op_simd_parser!(i8x16_min, 118);
unary_op_simd_parser!(u8x16_min, 119);
unary_op_simd_parser!(i8x16_max, 120);
unary_op_simd_parser!(u8x16_max, 121);

shift_instruction_parser!(i16x8_shl,139);
shift_instruction_parser!(i16x8_shr,140);
shift_instruction_parser!(u16x8_shr,141);


shift_instruction_parser!(i32x4_shl,171);
shift_instruction_parser!(i32x4_shr,172);
shift_instruction_parser!(u32x4_shr,173);
unary_op_simd_parser!(i32x4_add, 174);


shift_instruction_parser!(i64x2_shl,203);
shift_instruction_parser!(i64x2_shr,204);
shift_instruction_parser!(u64x2_shr,205);
unary_op_simd_parser!(i64x2_add, 206);

binary_op_simd_parser!(f32x4_abs, 224);
binary_op_simd_parser!(i32x4_abs, 160);
unary_op_simd_parser!(f32x4_mul, 230);
unary_op_simd_parser!(f32x4_div, 231);
unary_op_simd_parser!(f32x4_min, 232);
unary_op_simd_parser!(f32x4_max, 233);
unary_op_simd_parser!(f32x4_pmin, 234);
unary_op_simd_parser!(f32x4_pmax, 235);

binary_op_simd_parser!(i32x4_trunc_sat_f32x4_s, 248);
binary_op_simd_parser!(f32x4_convert_i32x4_u, 251);
