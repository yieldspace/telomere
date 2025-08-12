use binary_reader::BinaryReader;

use super::{instruction_generator::InstructionGenerator, type_checker::TypeChecker};

pub(crate) struct SimdParserContext<'a, R: BinaryReader> {
    pub(crate) instrs: &'a mut InstructionGenerator,
    pub(crate) checker: &'a mut TypeChecker,
    pub(crate) reader: &'a mut R,
}
mod prelude {
    pub(crate) use super::SimdParserContext;
    pub(crate) use crate::common::ValType;
    pub(crate) use crate::{WasmParserError, common::Operand, parser::core::values, runtime::vm};
    pub(crate) use binary_reader::BinaryReader;
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
