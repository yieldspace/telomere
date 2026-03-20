use crate::binary::BinaryReader;
use crate::common::{MemArg, MemType, Op, Operand};
use crate::WasmParserError;
use vstd::prelude::*;

use super::{instruction_generator::InstructionGenerator, type_checker::TypeChecker};

verus! {

#[inline(always)]
fn select_default_memory_family(shared: bool) -> (result: bool)
    ensures
        result == shared,
{
    shared
}

} // verus!

pub(crate) struct SimdParserContext<'a, R: BinaryReader> {
    pub(crate) mems: &'a [MemType],
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

fn memory_shared<R: BinaryReader>(
    ctx: &SimdParserContext<R>,
    memidx: u32,
) -> Result<bool, WasmParserError> {
    Ok(ctx
        .mems
        .get(memidx as usize)
        .ok_or(WasmParserError::InvalidMemIdx(memidx))?
        .shared)
}

fn select_memory_op<R: BinaryReader>(
    ctx: &SimdParserContext<R>,
    memidx: u32,
    local: Op,
    shared: Op,
    indexed_local: Op,
    indexed_shared: Op,
) -> Result<Op, WasmParserError> {
    Ok(
        match (
            memidx == 0,
            select_default_memory_family(memory_shared(ctx, memidx)?),
        ) {
            (true, false) => local,
            (true, true) => shared,
            (false, false) => indexed_local,
            (false, true) => indexed_shared,
        },
    )
}

fn push_memarg_instruction<R: BinaryReader>(
    ctx: &mut SimdParserContext<R>,
    memidx: u32,
    memarg: MemArg,
    local: Op,
    shared: Op,
    indexed_local: Op,
    indexed_shared: Op,
) -> Result<(), WasmParserError> {
    let op = select_memory_op(ctx, memidx, local, shared, indexed_local, indexed_shared)?;
    if memidx == 0 {
        ctx.instrs.push_with_operand(op, &[Operand { memarg }]);
    } else {
        ctx.instrs
            .push_with_operand(op, &[Operand { memarg }, Operand { u32: memidx }]);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn push_memarg_lane_instruction<R: BinaryReader>(
    ctx: &mut SimdParserContext<R>,
    memidx: u32,
    memarg: MemArg,
    lane: u32,
    local: Op,
    shared: Op,
    indexed_local: Op,
    indexed_shared: Op,
) -> Result<(), WasmParserError> {
    let op = select_memory_op(ctx, memidx, local, shared, indexed_local, indexed_shared)?;
    if memidx == 0 {
        ctx.instrs
            .push_with_operand(op, &[Operand { memarg }, Operand { u32: lane }]);
    } else {
        ctx.instrs.push_with_operand(
            op,
            &[
                Operand { memarg },
                Operand { u32: lane },
                Operand { u32: memidx },
            ],
        );
    }
    Ok(())
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

fn validate_lane(code: u32, lane: u8, lane_count: u8) -> Result<(), WasmParserError> {
    if lane >= lane_count {
        Err(WasmParserError::InvalidInstruction([
            0xFD, code as u8, lane, 0,
        ]))?
    }
    Ok(())
}

macro_rules! splat_instruction_parser {
    ($name: ident,$code: expr,$input: expr) => {
        pub(crate) mod $name {
            use super::prelude::*;
            pub(crate) const CODE: u32 = $code;
            pub(crate) fn parse<R: BinaryReader>(
                ctx: &mut SimdParserContext<R>,
            ) -> Result<usize, WasmParserError> {
                ctx.checker.op(&[$input], &[ValType::V128])?;
                ctx.instrs.push_instr1(vm::simd::$name);
                Ok(0)
            }
        }
    };
}

macro_rules! extract_lane_parser {
    ($name: ident,$code: expr,$output: expr,$lane_count: expr) => {
        pub(crate) mod $name {
            use super::prelude::*;
            pub(crate) const CODE: u32 = $code;
            pub(crate) fn parse<R: BinaryReader>(
                ctx: &mut SimdParserContext<R>,
            ) -> Result<usize, WasmParserError> {
                let (len, lane) = values::parse_byte(ctx.reader)?;
                super::validate_lane(CODE, lane, $lane_count)?;
                ctx.checker.op(&[ValType::V128], &[$output])?;
                ctx.instrs
                    .push_with_operand(vm::simd::$name, &[Operand { u32: lane as u32 }]);
                Ok(len)
            }
        }
    };
}

macro_rules! replace_lane_parser {
    ($name: ident,$code: expr,$input: expr,$lane_count: expr) => {
        pub(crate) mod $name {
            use super::prelude::*;
            pub(crate) const CODE: u32 = $code;
            pub(crate) fn parse<R: BinaryReader>(
                ctx: &mut SimdParserContext<R>,
            ) -> Result<usize, WasmParserError> {
                let (len, lane) = values::parse_byte(ctx.reader)?;
                super::validate_lane(CODE, lane, $lane_count)?;
                ctx.checker.op(&[ValType::V128, $input], &[ValType::V128])?;
                ctx.instrs
                    .push_with_operand(vm::simd::$name, &[Operand { u32: lane as u32 }]);
                Ok(len)
            }
        }
    };
}

macro_rules! load_lane_parser {
    ($name: ident,$shared: ident,$indexed_local: ident,$indexed_shared: ident,$code: expr,$natural_align: expr,$lane_count: expr) => {
        pub(crate) mod $name {
            use super::prelude::*;
            pub(crate) const CODE: u32 = $code;
            pub(crate) fn parse<R: BinaryReader>(
                ctx: &mut SimdParserContext<R>,
            ) -> Result<usize, WasmParserError> {
                let (len, memidx, memarg) = values::parse_memarg(ctx.reader, $natural_align)?;
                let (len2, lane) = values::parse_byte(ctx.reader)?;
                super::validate_lane(CODE, lane, $lane_count)?;
                ctx.checker
                    .op(&[ValType::I32, ValType::V128], &[ValType::V128])?;
                super::push_memarg_lane_instruction(
                    ctx,
                    memidx,
                    memarg,
                    lane as u32,
                    vm::simd::$name,
                    vm::simd::$shared,
                    vm::simd::$indexed_local,
                    vm::simd::$indexed_shared,
                )?;
                Ok(len + len2)
            }
        }
    };
}

macro_rules! store_lane_parser {
    ($name: ident,$shared: ident,$indexed_local: ident,$indexed_shared: ident,$code: expr,$natural_align: expr,$lane_count: expr) => {
        pub(crate) mod $name {
            use super::prelude::*;
            pub(crate) const CODE: u32 = $code;
            pub(crate) fn parse<R: BinaryReader>(
                ctx: &mut SimdParserContext<R>,
            ) -> Result<usize, WasmParserError> {
                let (len, memidx, memarg) = values::parse_memarg(ctx.reader, $natural_align)?;
                let (len2, lane) = values::parse_byte(ctx.reader)?;
                super::validate_lane(CODE, lane, $lane_count)?;
                ctx.checker.op(&[ValType::I32, ValType::V128], &[])?;
                super::push_memarg_lane_instruction(
                    ctx,
                    memidx,
                    memarg,
                    lane as u32,
                    vm::simd::$name,
                    vm::simd::$shared,
                    vm::simd::$indexed_local,
                    vm::simd::$indexed_shared,
                )?;
                Ok(len + len2)
            }
        }
    };
}

macro_rules! load_zero_parser {
    ($name: ident,$shared: ident,$indexed_local: ident,$indexed_shared: ident,$code: expr,$natural_align: expr) => {
        pub(crate) mod $name {
            use super::prelude::*;
            pub(crate) const CODE: u32 = $code;
            pub(crate) fn parse<R: BinaryReader>(
                ctx: &mut SimdParserContext<R>,
            ) -> Result<usize, WasmParserError> {
                let (len, memidx, memarg) = values::parse_memarg(ctx.reader, $natural_align)?;
                ctx.checker.load_op(ValType::V128)?;
                super::push_memarg_instruction(
                    ctx,
                    memidx,
                    memarg,
                    vm::simd::$name,
                    vm::simd::$shared,
                    vm::simd::$indexed_local,
                    vm::simd::$indexed_shared,
                )?;
                Ok(len)
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
        let (len, memidx, memarg) = values::parse_memarg(ctx.reader, 4)?; // TODO:
        ctx.checker.load_op(ValType::V128)?;
        super::push_memarg_instruction(
            ctx,
            memidx,
            memarg,
            vm::simd::op_v128_load,
            vm::simd::op_v128_load_shared,
            vm::simd::op_v128_load_indexed_local,
            vm::simd::op_v128_load_indexed_shared,
        )?;
        Ok(len)
    }
}

pub(crate) mod v128_load8x8_s {
    use super::prelude::*;
    pub(crate) const CODE: u32 = 1;
    pub(crate) fn parse<R: BinaryReader>(
        ctx: &mut SimdParserContext<R>,
    ) -> Result<usize, WasmParserError> {
        let (len, memidx, memarg) = values::parse_memarg(ctx.reader, 8)?;
        ctx.checker.load_op(ValType::V128)?;
        super::push_memarg_instruction(
            ctx,
            memidx,
            memarg,
            vm::simd::v128_load8x8_s,
            vm::simd::v128_load8x8_s_shared,
            vm::simd::v128_load8x8_s_indexed_local,
            vm::simd::v128_load8x8_s_indexed_shared,
        )?;
        Ok(len)
    }
}

pub(crate) mod v128_load8x8_u {
    use super::prelude::*;
    pub(crate) const CODE: u32 = 2;
    pub(crate) fn parse<R: BinaryReader>(
        ctx: &mut SimdParserContext<R>,
    ) -> Result<usize, WasmParserError> {
        let (len, memidx, memarg) = values::parse_memarg(ctx.reader, 8)?;
        ctx.checker.load_op(ValType::V128)?;
        super::push_memarg_instruction(
            ctx,
            memidx,
            memarg,
            vm::simd::v128_load8x8_u,
            vm::simd::v128_load8x8_u_shared,
            vm::simd::v128_load8x8_u_indexed_local,
            vm::simd::v128_load8x8_u_indexed_shared,
        )?;
        Ok(len)
    }
}

pub(crate) mod v128_load16x4_s {
    use super::prelude::*;
    pub(crate) const CODE: u32 = 3;
    pub(crate) fn parse<R: BinaryReader>(
        ctx: &mut SimdParserContext<R>,
    ) -> Result<usize, WasmParserError> {
        let (len, memidx, memarg) = values::parse_memarg(ctx.reader, 8)?;
        ctx.checker.load_op(ValType::V128)?;
        super::push_memarg_instruction(
            ctx,
            memidx,
            memarg,
            vm::simd::v128_load16x4_s,
            vm::simd::v128_load16x4_s_shared,
            vm::simd::v128_load16x4_s_indexed_local,
            vm::simd::v128_load16x4_s_indexed_shared,
        )?;
        Ok(len)
    }
}

pub(crate) mod v128_load16x4_u {
    use super::prelude::*;
    pub(crate) const CODE: u32 = 4;
    pub(crate) fn parse<R: BinaryReader>(
        ctx: &mut SimdParserContext<R>,
    ) -> Result<usize, WasmParserError> {
        let (len, memidx, memarg) = values::parse_memarg(ctx.reader, 8)?;
        ctx.checker.load_op(ValType::V128)?;
        super::push_memarg_instruction(
            ctx,
            memidx,
            memarg,
            vm::simd::v128_load16x4_u,
            vm::simd::v128_load16x4_u_shared,
            vm::simd::v128_load16x4_u_indexed_local,
            vm::simd::v128_load16x4_u_indexed_shared,
        )?;
        Ok(len)
    }
}

pub(crate) mod v128_load32x2_s {
    use super::prelude::*;
    pub(crate) const CODE: u32 = 5;
    pub(crate) fn parse<R: BinaryReader>(
        ctx: &mut SimdParserContext<R>,
    ) -> Result<usize, WasmParserError> {
        let (len, memidx, memarg) = values::parse_memarg(ctx.reader, 8)?;
        ctx.checker.load_op(ValType::V128)?;
        super::push_memarg_instruction(
            ctx,
            memidx,
            memarg,
            vm::simd::v128_load32x2_s,
            vm::simd::v128_load32x2_s_shared,
            vm::simd::v128_load32x2_s_indexed_local,
            vm::simd::v128_load32x2_s_indexed_shared,
        )?;
        Ok(len)
    }
}

pub(crate) mod v128_load32x2_u {
    use super::prelude::*;
    pub(crate) const CODE: u32 = 6;
    pub(crate) fn parse<R: BinaryReader>(
        ctx: &mut SimdParserContext<R>,
    ) -> Result<usize, WasmParserError> {
        let (len, memidx, memarg) = values::parse_memarg(ctx.reader, 8)?;
        ctx.checker.load_op(ValType::V128)?;
        super::push_memarg_instruction(
            ctx,
            memidx,
            memarg,
            vm::simd::v128_load32x2_u,
            vm::simd::v128_load32x2_u_shared,
            vm::simd::v128_load32x2_u_indexed_local,
            vm::simd::v128_load32x2_u_indexed_shared,
        )?;
        Ok(len)
    }
}

pub(crate) mod v128_load8_splat {
    use super::prelude::*;
    pub(crate) const CODE: u32 = 7;
    pub(crate) fn parse<R: BinaryReader>(
        ctx: &mut SimdParserContext<R>,
    ) -> Result<usize, WasmParserError> {
        let (len, memidx, memarg) = values::parse_memarg(ctx.reader, 8)?;
        ctx.checker.load_op(ValType::V128)?;
        super::push_memarg_instruction(
            ctx,
            memidx,
            memarg,
            vm::simd::v128_load8_splat,
            vm::simd::v128_load8_splat_shared,
            vm::simd::v128_load8_splat_indexed_local,
            vm::simd::v128_load8_splat_indexed_shared,
        )?;
        Ok(len)
    }
}

pub(crate) mod v128_load16_splat {
    use super::prelude::*;
    pub(crate) const CODE: u32 = 8;
    pub(crate) fn parse<R: BinaryReader>(
        ctx: &mut SimdParserContext<R>,
    ) -> Result<usize, WasmParserError> {
        let (len, memidx, memarg) = values::parse_memarg(ctx.reader, 8)?;
        ctx.checker.load_op(ValType::V128)?;
        super::push_memarg_instruction(
            ctx,
            memidx,
            memarg,
            vm::simd::v128_load16_splat,
            vm::simd::v128_load16_splat_shared,
            vm::simd::v128_load16_splat_indexed_local,
            vm::simd::v128_load16_splat_indexed_shared,
        )?;
        Ok(len)
    }
}
pub(crate) mod v128_load32_splat {
    use super::prelude::*;
    pub(crate) const CODE: u32 = 9;
    pub(crate) fn parse<R: BinaryReader>(
        ctx: &mut SimdParserContext<R>,
    ) -> Result<usize, WasmParserError> {
        let (len, memidx, memarg) = values::parse_memarg(ctx.reader, 8)?;
        ctx.checker.load_op(ValType::V128)?;
        super::push_memarg_instruction(
            ctx,
            memidx,
            memarg,
            vm::simd::v128_load32_splat,
            vm::simd::v128_load32_splat_shared,
            vm::simd::v128_load32_splat_indexed_local,
            vm::simd::v128_load32_splat_indexed_shared,
        )?;
        Ok(len)
    }
}
pub(crate) mod v128_load64_splat {
    use super::prelude::*;
    pub(crate) const CODE: u32 = 10;
    pub(crate) fn parse<R: BinaryReader>(
        ctx: &mut SimdParserContext<R>,
    ) -> Result<usize, WasmParserError> {
        let (len, memidx, memarg) = values::parse_memarg(ctx.reader, 8)?;
        ctx.checker.load_op(ValType::V128)?;
        super::push_memarg_instruction(
            ctx,
            memidx,
            memarg,
            vm::simd::v128_load64_splat,
            vm::simd::v128_load64_splat_shared,
            vm::simd::v128_load64_splat_indexed_local,
            vm::simd::v128_load64_splat_indexed_shared,
        )?;
        Ok(len)
    }
}

pub(crate) mod v128_store {
    use super::prelude::*;

    pub(crate) const CODE: u32 = 11;
    pub(crate) fn parse<R: BinaryReader>(
        ctx: &mut SimdParserContext<R>,
    ) -> Result<usize, WasmParserError> {
        let (len, memidx, memarg) = values::parse_memarg(ctx.reader, 4)?; // TODO:
        ctx.checker.store_op(ValType::V128)?;
        super::push_memarg_instruction(
            ctx,
            memidx,
            memarg,
            vm::simd::v128_store,
            vm::simd::v128_store_shared,
            vm::simd::v128_store_indexed_local,
            vm::simd::v128_store_indexed_shared,
        )?;
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

pub(crate) mod i8x16_shuffle {
    use super::prelude::*;
    pub(crate) const CODE: u32 = 13;
    pub(crate) fn parse<R: BinaryReader>(
        ctx: &mut SimdParserContext<R>,
    ) -> Result<usize, WasmParserError> {
        let lanes = ctx.reader.read_exact::<16>()?;
        for lane in lanes {
            super::validate_lane(CODE, lane, 32)?;
        }
        let mut left = [0u8; 8];
        let mut right = [0u8; 8];
        left.copy_from_slice(&lanes[0..8]);
        right.copy_from_slice(&lanes[8..16]);
        ctx.checker
            .op(&[ValType::V128, ValType::V128], &[ValType::V128])?;
        ctx.instrs.push_with_operand(
            vm::simd::i8x16_shuffle,
            &[Operand { encoded: left }, Operand { encoded: right }],
        );
        Ok(16)
    }
}
unary_op_simd_parser!(i8x16_swizzle, 14);
splat_instruction_parser!(i8x16_splat, 15, ValType::I32);
splat_instruction_parser!(i16x8_splat, 16, ValType::I32);
splat_instruction_parser!(i32x4_splat, 17, ValType::I32);
splat_instruction_parser!(i64x2_splat, 18, ValType::I64);
splat_instruction_parser!(f32x4_splat, 19, ValType::F32);
splat_instruction_parser!(f64x2_splat, 20, ValType::F64);

pub(crate) mod i8x16_extract_lane_s {
    use super::prelude::*;
    pub(crate) const CODE: u32 = 21;
    pub(crate) fn parse<R: BinaryReader>(
        ctx: &mut SimdParserContext<R>,
    ) -> Result<usize, WasmParserError> {
        let (len, lane) = values::parse_byte(ctx.reader)?;
        super::validate_lane(CODE, lane, 16)?;
        ctx.checker.op(&[ValType::V128], &[ValType::I32])?;

        ctx.instrs.push_with_operand(
            vm::simd::op_i8x16_extract_lane_s,
            &[Operand { u32: lane as u32 }],
        );
        Ok(len)
    }
}
extract_lane_parser!(i8x16_extract_lane_u, 22, ValType::I32, 16);
replace_lane_parser!(i8x16_replace_lane, 23, ValType::I32, 16);
extract_lane_parser!(i16x8_extract_lane_s, 24, ValType::I32, 8);
extract_lane_parser!(i16x8_extract_lane_u, 25, ValType::I32, 8);
replace_lane_parser!(i16x8_replace_lane, 26, ValType::I32, 8);
extract_lane_parser!(i32x4_extract_lane, 27, ValType::I32, 4);
replace_lane_parser!(i32x4_replace_lane, 28, ValType::I32, 4);
extract_lane_parser!(i64x2_extract_lane, 29, ValType::I64, 2);
replace_lane_parser!(i64x2_replace_lane, 30, ValType::I64, 2);
extract_lane_parser!(f32x4_extract_lane, 31, ValType::F32, 4);
replace_lane_parser!(f32x4_replace_lane, 32, ValType::F32, 4);
extract_lane_parser!(f64x2_extract_lane, 33, ValType::F64, 2);
replace_lane_parser!(f64x2_replace_lane, 34, ValType::F64, 2);
unary_op_simd_parser!(i8x16_eq, 35);
unary_op_simd_parser!(i8x16_ne, 36);
unary_op_simd_parser!(i8x16_lt, 37);
unary_op_simd_parser!(u8x16_lt, 38);
unary_op_simd_parser!(i8x16_gt, 39);
unary_op_simd_parser!(u8x16_gt, 40);
unary_op_simd_parser!(i8x16_le, 41);
unary_op_simd_parser!(u8x16_le, 42);
unary_op_simd_parser!(i8x16_ge, 43);
unary_op_simd_parser!(u8x16_ge, 44);
unary_op_simd_parser!(i16x8_eq, 45);
unary_op_simd_parser!(i16x8_ne, 46);
unary_op_simd_parser!(i16x8_lt, 47);
unary_op_simd_parser!(u16x8_lt, 48);
unary_op_simd_parser!(i16x8_gt, 49);
unary_op_simd_parser!(u16x8_gt, 50);
unary_op_simd_parser!(i16x8_le, 51);
unary_op_simd_parser!(u16x8_le, 52);
unary_op_simd_parser!(i16x8_ge, 53);
unary_op_simd_parser!(u16x8_ge, 54);
unary_op_simd_parser!(i32x4_eq, 55);
unary_op_simd_parser!(i32x4_ne, 56);
unary_op_simd_parser!(i32x4_lt, 57);
unary_op_simd_parser!(u32x4_lt, 58);
unary_op_simd_parser!(i32x4_gt, 59);
unary_op_simd_parser!(u32x4_gt, 60);
unary_op_simd_parser!(i32x4_le, 61);
unary_op_simd_parser!(u32x4_le, 62);
unary_op_simd_parser!(i32x4_ge, 63);
unary_op_simd_parser!(u32x4_ge, 64);
unary_op_simd_parser!(f32x4_eq, 65);
unary_op_simd_parser!(f32x4_ne, 66);
unary_op_simd_parser!(f32x4_lt, 67);
unary_op_simd_parser!(f32x4_gt, 68);
unary_op_simd_parser!(f32x4_le, 69);
unary_op_simd_parser!(f32x4_ge, 70);
unary_op_simd_parser!(f64x2_eq, 71);
unary_op_simd_parser!(f64x2_ne, 72);
unary_op_simd_parser!(f64x2_lt, 73);
unary_op_simd_parser!(f64x2_gt, 74);
unary_op_simd_parser!(f64x2_le, 75);
unary_op_simd_parser!(f64x2_ge, 76);
binary_op_simd_parser!(v128_not, 77);
unary_op_simd_parser!(v128_and, 78);
unary_op_simd_parser!(v128_andnot, 79);
unary_op_simd_parser!(v128_or, 80);
unary_op_simd_parser!(v128_xor, 81);
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

pub(crate) mod v128_any_true {
    use super::prelude::*;
    pub(crate) const CODE: u32 = 83;
    pub(crate) fn parse<R: BinaryReader>(
        ctx: &mut SimdParserContext<R>,
    ) -> Result<usize, WasmParserError> {
        ctx.checker.op(&[ValType::V128], &[ValType::I32])?;
        ctx.instrs.push_instr1(vm::simd::v128_any_true);
        Ok(0)
    }
}
load_lane_parser!(
    v128_load8_lane,
    v128_load8_lane_shared,
    v128_load8_lane_indexed_local,
    v128_load8_lane_indexed_shared,
    84,
    0,
    16
);
load_lane_parser!(
    v128_load16_lane,
    v128_load16_lane_shared,
    v128_load16_lane_indexed_local,
    v128_load16_lane_indexed_shared,
    85,
    1,
    8
);
load_lane_parser!(
    v128_load32_lane,
    v128_load32_lane_shared,
    v128_load32_lane_indexed_local,
    v128_load32_lane_indexed_shared,
    86,
    2,
    4
);
load_lane_parser!(
    v128_load64_lane,
    v128_load64_lane_shared,
    v128_load64_lane_indexed_local,
    v128_load64_lane_indexed_shared,
    87,
    3,
    2
);
store_lane_parser!(
    v128_store8_lane,
    v128_store8_lane_shared,
    v128_store8_lane_indexed_local,
    v128_store8_lane_indexed_shared,
    88,
    0,
    16
);
store_lane_parser!(
    v128_store16_lane,
    v128_store16_lane_shared,
    v128_store16_lane_indexed_local,
    v128_store16_lane_indexed_shared,
    89,
    1,
    8
);
store_lane_parser!(
    v128_store32_lane,
    v128_store32_lane_shared,
    v128_store32_lane_indexed_local,
    v128_store32_lane_indexed_shared,
    90,
    2,
    4
);
store_lane_parser!(
    v128_store64_lane,
    v128_store64_lane_shared,
    v128_store64_lane_indexed_local,
    v128_store64_lane_indexed_shared,
    91,
    3,
    2
);
load_zero_parser!(
    v128_load32_zero,
    v128_load32_zero_shared,
    v128_load32_zero_indexed_local,
    v128_load32_zero_indexed_shared,
    92,
    2
);
load_zero_parser!(
    v128_load64_zero,
    v128_load64_zero_shared,
    v128_load64_zero_indexed_local,
    v128_load64_zero_indexed_shared,
    93,
    3
);

binary_op_simd_parser!(f32x4_demote_f64x2_zero, 94);
binary_op_simd_parser!(f64x2_promote_low_f32x4, 95);
binary_op_simd_parser!(i8x16_abs, 96);
binary_op_simd_parser!(i8x16_neg, 97);
binary_op_simd_parser!(u8x16_popcnt, 98);
pub(crate) mod i8x16_all_true {
    use super::prelude::*;
    pub(crate) const CODE: u32 = 99;
    pub(crate) fn parse<R: BinaryReader>(
        ctx: &mut SimdParserContext<R>,
    ) -> Result<usize, WasmParserError> {
        ctx.checker.op(&[ValType::V128], &[ValType::I32])?;
        ctx.instrs.push_instr1(vm::simd::i8x16_all_true);
        Ok(0)
    }
}
pub(crate) mod i8x16_bitmask {
    use super::prelude::*;
    pub(crate) const CODE: u32 = 100;
    pub(crate) fn parse<R: BinaryReader>(
        ctx: &mut SimdParserContext<R>,
    ) -> Result<usize, WasmParserError> {
        ctx.checker.op(&[ValType::V128], &[ValType::I32])?;
        ctx.instrs.push_instr1(vm::simd::i8x16_bitmask);
        Ok(0)
    }
}
unary_op_simd_parser!(i8x16_narrow_i16x8_s, 101);
unary_op_simd_parser!(i8x16_narrow_i16x8_u, 102);
binary_op_simd_parser!(f32x4_ceil, 103);
binary_op_simd_parser!(f32x4_floor, 104);
binary_op_simd_parser!(f32x4_trunc, 105);
binary_op_simd_parser!(f32x4_nearest, 106);
shift_instruction_parser!(i8x16_shl, 107);
shift_instruction_parser!(i8x16_shr, 108);
shift_instruction_parser!(u8x16_shr, 109);

unary_op_simd_parser!(i8x16_add, 110);
unary_op_simd_parser!(i8x16_add_sat, 111);
unary_op_simd_parser!(u8x16_add_sat, 112);
unary_op_simd_parser!(i8x16_sub, 113);
unary_op_simd_parser!(i8x16_sub_sat, 114);
unary_op_simd_parser!(u8x16_sub_sat, 115);
binary_op_simd_parser!(f64x2_ceil, 116);
binary_op_simd_parser!(f64x2_floor, 117);
unary_op_simd_parser!(i8x16_min, 118);
unary_op_simd_parser!(u8x16_min, 119);
unary_op_simd_parser!(i8x16_max, 120);
unary_op_simd_parser!(u8x16_max, 121);
binary_op_simd_parser!(f64x2_trunc, 122);
unary_op_simd_parser!(u8x16_avgr, 123);
binary_op_simd_parser!(i16x8_extadd_pairwise_i8x16, 124);
binary_op_simd_parser!(u16x8_extadd_pairwise_i8x16, 125);
binary_op_simd_parser!(i32x4_extadd_pairwise_i16x8, 126);
binary_op_simd_parser!(u32x4_extadd_pairwise_i16x8, 127);

binary_op_simd_parser!(i16x8_abs, 128);
binary_op_simd_parser!(i16x8_neg, 129);
unary_op_simd_parser!(i16x8_q15mulr_sat_s, 130);
pub(crate) mod i16x8_all_true {
    use super::prelude::*;
    pub(crate) const CODE: u32 = 131;
    pub(crate) fn parse<R: BinaryReader>(
        ctx: &mut SimdParserContext<R>,
    ) -> Result<usize, WasmParserError> {
        ctx.checker.op(&[ValType::V128], &[ValType::I32])?;
        ctx.instrs.push_instr1(vm::simd::i16x8_all_true);
        Ok(0)
    }
}
pub(crate) mod i16x8_bitmask {
    use super::prelude::*;
    pub(crate) const CODE: u32 = 132;
    pub(crate) fn parse<R: BinaryReader>(
        ctx: &mut SimdParserContext<R>,
    ) -> Result<usize, WasmParserError> {
        ctx.checker.op(&[ValType::V128], &[ValType::I32])?;
        ctx.instrs.push_instr1(vm::simd::i16x8_bitmask);
        Ok(0)
    }
}
unary_op_simd_parser!(i16x8_narrow_i32x4_s, 133);
unary_op_simd_parser!(i16x8_narrow_i32x4_u, 134);
binary_op_simd_parser!(i16x8_extend_low_i8x16_s, 135);
binary_op_simd_parser!(i16x8_extend_high_i8x16_s, 136);
binary_op_simd_parser!(i16x8_extend_low_i8x16_u, 137);
binary_op_simd_parser!(i16x8_extend_high_i8x16_u, 138);
shift_instruction_parser!(i16x8_shl, 139);
shift_instruction_parser!(i16x8_shr, 140);
shift_instruction_parser!(u16x8_shr, 141);
unary_op_simd_parser!(i16x8_add, 142);
unary_op_simd_parser!(i16x8_add_sat, 143);
unary_op_simd_parser!(u16x8_add_sat, 144);
unary_op_simd_parser!(i16x8_sub, 145);
unary_op_simd_parser!(i16x8_sub_sat, 146);
unary_op_simd_parser!(u16x8_sub_sat, 147);
binary_op_simd_parser!(f64x2_nearest, 148);
unary_op_simd_parser!(i16x8_mul, 149);
unary_op_simd_parser!(i16x8_min, 150);
unary_op_simd_parser!(u16x8_min, 151);
unary_op_simd_parser!(i16x8_max, 152);
unary_op_simd_parser!(u16x8_max, 153);
unary_op_simd_parser!(u16x8_avgr, 155);
unary_op_simd_parser!(i16x8_extmul_low, 156);
unary_op_simd_parser!(i16x8_extmul_high, 157);
unary_op_simd_parser!(u16x8_extmul_low, 158);
unary_op_simd_parser!(u16x8_extmul_high, 159);
binary_op_simd_parser!(i32x4_abs, 160);
binary_op_simd_parser!(i32x4_neg, 161);

pub(crate) mod i32x4_all_true {
    use super::prelude::*;
    pub(crate) const CODE: u32 = 163;
    pub(crate) fn parse<R: BinaryReader>(
        ctx: &mut SimdParserContext<R>,
    ) -> Result<usize, WasmParserError> {
        ctx.checker.op(&[ValType::V128], &[ValType::I32])?;
        ctx.instrs.push_instr1(vm::simd::i32x4_all_true);
        Ok(0)
    }
}

pub(crate) mod i32x4_bitmask {
    use super::prelude::*;
    pub(crate) const CODE: u32 = 164;
    pub(crate) fn parse<R: BinaryReader>(
        ctx: &mut SimdParserContext<R>,
    ) -> Result<usize, WasmParserError> {
        ctx.checker.op(&[ValType::V128], &[ValType::I32])?;
        ctx.instrs.push_instr1(vm::simd::i32x4_bitmask);
        Ok(0)
    }
}

binary_op_simd_parser!(i32x4_extend_low_i16x8_s, 167);
binary_op_simd_parser!(i32x4_extend_high_i16x8_s, 168);
binary_op_simd_parser!(i32x4_extend_low_i16x8_u, 169);
binary_op_simd_parser!(i32x4_extend_high_i16x8_u, 170);
shift_instruction_parser!(i32x4_shl, 171);
shift_instruction_parser!(i32x4_shr, 172);
shift_instruction_parser!(u32x4_shr, 173);
unary_op_simd_parser!(i32x4_add, 174);
unary_op_simd_parser!(i32x4_sub, 177);
unary_op_simd_parser!(i32x4_mul, 181);
unary_op_simd_parser!(i32x4_min, 182);
unary_op_simd_parser!(u32x4_min, 183);
unary_op_simd_parser!(i32x4_max, 184);
unary_op_simd_parser!(u32x4_max, 185);
unary_op_simd_parser!(i32x4_dot_i16x8, 186);

unary_op_simd_parser!(i32x4_extmul_low, 188);
unary_op_simd_parser!(i32x4_extmul_high, 189);
unary_op_simd_parser!(u32x4_extmul_low, 190);
unary_op_simd_parser!(u32x4_extmul_high, 191);
binary_op_simd_parser!(i64x2_abs, 192);
binary_op_simd_parser!(i64x2_neg, 193);

pub(crate) mod i64x2_all_true {
    use super::prelude::*;
    pub(crate) const CODE: u32 = 195;
    pub(crate) fn parse<R: BinaryReader>(
        ctx: &mut SimdParserContext<R>,
    ) -> Result<usize, WasmParserError> {
        ctx.checker.op(&[ValType::V128], &[ValType::I32])?;
        ctx.instrs.push_instr1(vm::simd::i64x2_all_true);
        Ok(0)
    }
}

pub(crate) mod i64x2_bitmask {
    use super::prelude::*;
    pub(crate) const CODE: u32 = 196;
    pub(crate) fn parse<R: BinaryReader>(
        ctx: &mut SimdParserContext<R>,
    ) -> Result<usize, WasmParserError> {
        ctx.checker.op(&[ValType::V128], &[ValType::I32])?;
        ctx.instrs.push_instr1(vm::simd::i64x2_bitmask);
        Ok(0)
    }
}

shift_instruction_parser!(i64x2_shl, 203);
shift_instruction_parser!(i64x2_shr, 204);
shift_instruction_parser!(u64x2_shr, 205);
unary_op_simd_parser!(i64x2_add, 206);
binary_op_simd_parser!(i64x2_extend_low_i32x4_s, 199);
binary_op_simd_parser!(i64x2_extend_high_i32x4_s, 200);
binary_op_simd_parser!(i64x2_extend_low_i32x4_u, 201);
binary_op_simd_parser!(i64x2_extend_high_i32x4_u, 202);
unary_op_simd_parser!(i64x2_sub, 209);
unary_op_simd_parser!(i64x2_mul, 213);
unary_op_simd_parser!(i64x2_eq, 214);
unary_op_simd_parser!(i64x2_ne, 215);
unary_op_simd_parser!(i64x2_lt, 216);
unary_op_simd_parser!(i64x2_gt, 217);
unary_op_simd_parser!(i64x2_le, 218);
unary_op_simd_parser!(i64x2_ge, 219);
unary_op_simd_parser!(i64x2_extmul_low_i32x4_s, 220);
unary_op_simd_parser!(i64x2_extmul_high_i32x4_s, 221);
unary_op_simd_parser!(i64x2_extmul_low_i32x4_u, 222);
unary_op_simd_parser!(i64x2_extmul_high_i32x4_u, 223);

binary_op_simd_parser!(f32x4_abs, 224);
binary_op_simd_parser!(f32x4_neg, 225);
binary_op_simd_parser!(f32x4_sqrt, 227);
unary_op_simd_parser!(f32x4_add, 228);
unary_op_simd_parser!(f32x4_sub, 229);
unary_op_simd_parser!(f32x4_mul, 230);
unary_op_simd_parser!(f32x4_div, 231);
unary_op_simd_parser!(f32x4_min, 232);
unary_op_simd_parser!(f32x4_max, 233);
unary_op_simd_parser!(f32x4_pmin, 234);
unary_op_simd_parser!(f32x4_pmax, 235);

binary_op_simd_parser!(f64x2_abs, 236);
binary_op_simd_parser!(f64x2_neg, 237);
binary_op_simd_parser!(f64x2_sqrt, 239);
unary_op_simd_parser!(f64x2_add, 240);
unary_op_simd_parser!(f64x2_sub, 241);
unary_op_simd_parser!(f64x2_mul, 242);
unary_op_simd_parser!(f64x2_div, 243);
unary_op_simd_parser!(f64x2_min, 244);
unary_op_simd_parser!(f64x2_max, 245);
unary_op_simd_parser!(f64x2_pmin, 246);
unary_op_simd_parser!(f64x2_pmax, 247);

binary_op_simd_parser!(i32x4_trunc_sat_f32x4_s, 248);
binary_op_simd_parser!(i32x4_trunc_sat_f32x4_u, 249);
binary_op_simd_parser!(f32x4_convert_i32x4_s, 250);
binary_op_simd_parser!(f32x4_convert_i32x4_u, 251);
binary_op_simd_parser!(i32x4_trunc_sat_f64x2_s, 252);
binary_op_simd_parser!(i32x4_trunc_sat_f64x2_u, 253);
binary_op_simd_parser!(f64x2_convert_low_i32x4_s, 254);
binary_op_simd_parser!(f64x2_convert_low_i32x4_u, 255);
