use super::*;

#[derive(Clone)]
pub(super) struct DecodedInstruction {
    pub(super) old_range: Range<usize>,
    pub(super) kind: DecodedKind,
    pub(super) raw: Box<[Instr]>,
}

#[derive(Clone, Copy, Debug)]
pub(super) enum TypedConst {
    I32(i32),
    I64(i64),
    F32(u32),
    F64(u64),
}

impl TypedConst {
    pub(super) fn width(self) -> ValueSize {
        match self {
            Self::I32(_) | Self::F32(_) => ValueSize::Byte4,
            Self::I64(_) | Self::F64(_) => ValueSize::Byte8,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum TypedScalarOp {
    I32(I32ScalarKind),
    I64(I64ScalarKind),
    F32(FloatScalarKind),
    F64(FloatScalarKind),
}

impl TypedScalarOp {
    pub(super) fn width(self) -> ValueSize {
        match self {
            Self::I32(_) | Self::F32(_) => ValueSize::Byte4,
            Self::I64(_) | Self::F64(_) => ValueSize::Byte8,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum TypedCompareOp {
    I32(IntCompareKind),
    I64(IntCompareKind),
    F32(FloatCompareKind),
    F64(FloatCompareKind),
}

impl TypedCompareOp {
    pub(super) fn width(self) -> ValueSize {
        match self {
            Self::I32(_) | Self::F32(_) => ValueSize::Byte4,
            Self::I64(_) | Self::F64(_) => ValueSize::Byte8,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum TypedLoadOp {
    Bits4(Load4Kind),
    Bits8(Load8Kind),
}

impl TypedLoadOp {
    pub(super) fn width(self) -> ValueSize {
        match self {
            Self::Bits4(_) => ValueSize::Byte4,
            Self::Bits8(_) => ValueSize::Byte8,
        }
    }

    pub(super) fn uses_dedicated_const(self) -> bool {
        matches!(self, Self::Bits4(Load4Kind::I32))
    }

    pub(super) fn uses_dedicated_local_addr(self) -> bool {
        matches!(
            self,
            Self::Bits4(
                Load4Kind::I32
                    | Load4Kind::I32Load8U
                    | Load4Kind::I32Load16S
                    | Load4Kind::I32Load16U
                    | Load4Kind::F32
            )
        )
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum TypedStoreOp {
    Bits4(Store4Kind),
    Bits8(Store8Kind),
}

impl TypedStoreOp {
    pub(super) fn value_width(self) -> ValueSize {
        match self {
            Self::Bits4(_) => ValueSize::Byte4,
            Self::Bits8(_) => ValueSize::Byte8,
        }
    }

    pub(super) fn uses_dedicated_const(self) -> bool {
        matches!(self, Self::Bits4(Store4Kind::I32))
    }

    pub(super) fn uses_dedicated_local_local(self) -> bool {
        matches!(
            self,
            Self::Bits4(Store4Kind::I32 | Store4Kind::I32Store8 | Store4Kind::I32Store16)
        )
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum DecodedKind {
    Raw,
    Const(TypedConst),
    LocalGet(ValueSize, u32),
    LocalSet(ValueSize, u32),
    LocalTee(ValueSize, u32),
    Select(ValueSize),
    BrIf(u32),
    If(u32),
    Eqz(ValueSize),
    Scalar(TypedScalarOp),
    Compare(TypedCompareOp),
    Load(TypedLoadOp, MemArg),
    Store(TypedStoreOp, MemArg),
}

pub(super) fn decode_instructions(instrs: &[Instr], starts: &[usize]) -> Vec<DecodedInstruction> {
    let mut decoded = Vec::with_capacity(starts.len());
    for (index, &start) in starts.iter().enumerate() {
        let end = starts.get(index + 1).copied().unwrap_or(instrs.len());
        let raw = instrs[start..end].to_vec().into_boxed_slice();
        let kind = decode_kind(&raw);
        decoded.push(DecodedInstruction {
            old_range: start..end,
            kind,
            raw,
        });
    }
    decoded
}

pub(super) fn decode_kind(raw: &[Instr]) -> DecodedKind {
    let op = unsafe { raw[0].op };

    macro_rules! decode1 {
        ($vmop:path, $kind:expr) => {
            if raw.len() == 1 && std::ptr::fn_addr_eq(op, $vmop as crate::common::Op) {
                return $kind;
            }
        };
    }
    macro_rules! decode2 {
        ($vmop:path, $kind:expr) => {
            if raw.len() == 2 && std::ptr::fn_addr_eq(op, $vmop as crate::common::Op) {
                return $kind;
            }
        };
    }

    decode2!(
        vm::op_i32_const,
        DecodedKind::Const(TypedConst::I32(unsafe { raw[1].operand.i32 }))
    );
    decode2!(
        vm::op_i64_const,
        DecodedKind::Const(TypedConst::I64(unsafe { raw[1].operand.i64 }))
    );
    decode2!(
        vm::op_f32_const,
        DecodedKind::Const(TypedConst::F32(unsafe { raw[1].operand.f32 }.to_bits()))
    );
    decode2!(
        vm::op_f64_const,
        DecodedKind::Const(TypedConst::F64(unsafe { raw[1].operand.f64 }.to_bits()))
    );

    decode2!(
        vm::op_local_get4,
        DecodedKind::LocalGet(ValueSize::Byte4, unsafe { raw[1].operand.local_addr })
    );
    decode2!(
        vm::op_local_get8,
        DecodedKind::LocalGet(ValueSize::Byte8, unsafe { raw[1].operand.local_addr })
    );
    decode2!(
        vm::op_local_get16,
        DecodedKind::LocalGet(ValueSize::Byte16, unsafe { raw[1].operand.local_addr })
    );
    decode2!(
        vm::op_local_set4,
        DecodedKind::LocalSet(ValueSize::Byte4, unsafe { raw[1].operand.local_addr })
    );
    decode2!(
        vm::op_local_set8,
        DecodedKind::LocalSet(ValueSize::Byte8, unsafe { raw[1].operand.local_addr })
    );
    decode2!(
        vm::op_local_set16,
        DecodedKind::LocalSet(ValueSize::Byte16, unsafe { raw[1].operand.local_addr })
    );
    decode2!(
        vm::op_local_tee4,
        DecodedKind::LocalTee(ValueSize::Byte4, unsafe { raw[1].operand.local_addr })
    );
    decode2!(
        vm::op_local_tee8,
        DecodedKind::LocalTee(ValueSize::Byte8, unsafe { raw[1].operand.local_addr })
    );
    decode2!(
        vm::op_local_tee16,
        DecodedKind::LocalTee(ValueSize::Byte16, unsafe { raw[1].operand.local_addr })
    );

    decode2!(
        vm::op_br_if,
        DecodedKind::BrIf(unsafe { raw[1].operand.jump_addr })
    );
    decode2!(
        vm::op_if,
        DecodedKind::If(unsafe { raw[1].operand.jump_addr })
    );

    decode1!(vm::op_select4, DecodedKind::Select(ValueSize::Byte4));
    decode1!(vm::op_select8, DecodedKind::Select(ValueSize::Byte8));
    if raw.len() == 2 && std::ptr::fn_addr_eq(op, vm::op_select as crate::common::Op) {
        match unsafe { raw[1].operand.select } {
            4 => return DecodedKind::Select(ValueSize::Byte4),
            8 => return DecodedKind::Select(ValueSize::Byte8),
            _ => {}
        }
    }

    decode1!(vm::op_i32_eqz, DecodedKind::Eqz(ValueSize::Byte4));
    decode1!(vm::op_i64_eqz, DecodedKind::Eqz(ValueSize::Byte8));

    decode1!(
        vm::op_i32_add,
        DecodedKind::Scalar(TypedScalarOp::I32(I32ScalarKind::Add))
    );
    decode1!(
        vm::op_i32_sub,
        DecodedKind::Scalar(TypedScalarOp::I32(I32ScalarKind::Sub))
    );
    decode1!(
        vm::op_i32_mul,
        DecodedKind::Scalar(TypedScalarOp::I32(I32ScalarKind::Mul))
    );
    decode1!(
        vm::op_i32_and,
        DecodedKind::Scalar(TypedScalarOp::I32(I32ScalarKind::And))
    );
    decode1!(
        vm::op_i32_or,
        DecodedKind::Scalar(TypedScalarOp::I32(I32ScalarKind::Or))
    );
    decode1!(
        vm::op_i32_xor,
        DecodedKind::Scalar(TypedScalarOp::I32(I32ScalarKind::Xor))
    );
    decode1!(
        vm::op_i32_shl,
        DecodedKind::Scalar(TypedScalarOp::I32(I32ScalarKind::Shl))
    );
    decode1!(
        vm::op_i32_shr_s,
        DecodedKind::Scalar(TypedScalarOp::I32(I32ScalarKind::ShrS))
    );
    decode1!(
        vm::op_i32_shr_u,
        DecodedKind::Scalar(TypedScalarOp::I32(I32ScalarKind::ShrU))
    );
    decode1!(
        vm::op_i32_div_s,
        DecodedKind::Scalar(TypedScalarOp::I32(I32ScalarKind::DivS))
    );
    decode1!(
        vm::op_i32_div_u,
        DecodedKind::Scalar(TypedScalarOp::I32(I32ScalarKind::DivU))
    );
    decode1!(
        vm::op_i32_rem_s,
        DecodedKind::Scalar(TypedScalarOp::I32(I32ScalarKind::RemS))
    );
    decode1!(
        vm::op_i32_rem_u,
        DecodedKind::Scalar(TypedScalarOp::I32(I32ScalarKind::RemU))
    );

    decode1!(
        vm::op_i64_add,
        DecodedKind::Scalar(TypedScalarOp::I64(I64ScalarKind::Add))
    );
    decode1!(
        vm::op_i64_sub,
        DecodedKind::Scalar(TypedScalarOp::I64(I64ScalarKind::Sub))
    );
    decode1!(
        vm::op_i64_mul,
        DecodedKind::Scalar(TypedScalarOp::I64(I64ScalarKind::Mul))
    );
    decode1!(
        vm::op_i64_and,
        DecodedKind::Scalar(TypedScalarOp::I64(I64ScalarKind::And))
    );
    decode1!(
        vm::op_i64_or,
        DecodedKind::Scalar(TypedScalarOp::I64(I64ScalarKind::Or))
    );
    decode1!(
        vm::op_i64_xor,
        DecodedKind::Scalar(TypedScalarOp::I64(I64ScalarKind::Xor))
    );
    decode1!(
        vm::op_i64_shl,
        DecodedKind::Scalar(TypedScalarOp::I64(I64ScalarKind::Shl))
    );
    decode1!(
        vm::op_i64_shr_s,
        DecodedKind::Scalar(TypedScalarOp::I64(I64ScalarKind::ShrS))
    );
    decode1!(
        vm::op_i64_shr_u,
        DecodedKind::Scalar(TypedScalarOp::I64(I64ScalarKind::ShrU))
    );
    decode1!(
        vm::op_i64_div_s,
        DecodedKind::Scalar(TypedScalarOp::I64(I64ScalarKind::DivS))
    );
    decode1!(
        vm::op_i64_div_u,
        DecodedKind::Scalar(TypedScalarOp::I64(I64ScalarKind::DivU))
    );
    decode1!(
        vm::op_i64_rem_s,
        DecodedKind::Scalar(TypedScalarOp::I64(I64ScalarKind::RemS))
    );
    decode1!(
        vm::op_i64_rem_u,
        DecodedKind::Scalar(TypedScalarOp::I64(I64ScalarKind::RemU))
    );

    decode1!(
        vm::op_f32_add,
        DecodedKind::Scalar(TypedScalarOp::F32(FloatScalarKind::Add))
    );
    decode1!(
        vm::op_f32_sub,
        DecodedKind::Scalar(TypedScalarOp::F32(FloatScalarKind::Sub))
    );
    decode1!(
        vm::op_f32_mul,
        DecodedKind::Scalar(TypedScalarOp::F32(FloatScalarKind::Mul))
    );
    decode1!(
        vm::op_f32_div,
        DecodedKind::Scalar(TypedScalarOp::F32(FloatScalarKind::Div))
    );
    decode1!(
        vm::op_f64_add,
        DecodedKind::Scalar(TypedScalarOp::F64(FloatScalarKind::Add))
    );
    decode1!(
        vm::op_f64_sub,
        DecodedKind::Scalar(TypedScalarOp::F64(FloatScalarKind::Sub))
    );
    decode1!(
        vm::op_f64_mul,
        DecodedKind::Scalar(TypedScalarOp::F64(FloatScalarKind::Mul))
    );
    decode1!(
        vm::op_f64_div,
        DecodedKind::Scalar(TypedScalarOp::F64(FloatScalarKind::Div))
    );

    decode1!(
        vm::op_i32_eq,
        DecodedKind::Compare(TypedCompareOp::I32(IntCompareKind::Eq))
    );
    decode1!(
        vm::op_i32_ne,
        DecodedKind::Compare(TypedCompareOp::I32(IntCompareKind::Ne))
    );
    decode1!(
        vm::op_i32_lt_s,
        DecodedKind::Compare(TypedCompareOp::I32(IntCompareKind::LtS))
    );
    decode1!(
        vm::op_i32_lt_u,
        DecodedKind::Compare(TypedCompareOp::I32(IntCompareKind::LtU))
    );
    decode1!(
        vm::op_i32_gt_s,
        DecodedKind::Compare(TypedCompareOp::I32(IntCompareKind::GtS))
    );
    decode1!(
        vm::op_i32_gt_u,
        DecodedKind::Compare(TypedCompareOp::I32(IntCompareKind::GtU))
    );
    decode1!(
        vm::op_i32_le_s,
        DecodedKind::Compare(TypedCompareOp::I32(IntCompareKind::LeS))
    );
    decode1!(
        vm::op_i32_le_u,
        DecodedKind::Compare(TypedCompareOp::I32(IntCompareKind::LeU))
    );
    decode1!(
        vm::op_i32_ge_s,
        DecodedKind::Compare(TypedCompareOp::I32(IntCompareKind::GeS))
    );
    decode1!(
        vm::op_i32_ge_u,
        DecodedKind::Compare(TypedCompareOp::I32(IntCompareKind::GeU))
    );

    decode1!(
        vm::op_i64_eq,
        DecodedKind::Compare(TypedCompareOp::I64(IntCompareKind::Eq))
    );
    decode1!(
        vm::op_i64_ne,
        DecodedKind::Compare(TypedCompareOp::I64(IntCompareKind::Ne))
    );
    decode1!(
        vm::op_i64_lt_s,
        DecodedKind::Compare(TypedCompareOp::I64(IntCompareKind::LtS))
    );
    decode1!(
        vm::op_i64_lt_u,
        DecodedKind::Compare(TypedCompareOp::I64(IntCompareKind::LtU))
    );
    decode1!(
        vm::op_i64_gt_s,
        DecodedKind::Compare(TypedCompareOp::I64(IntCompareKind::GtS))
    );
    decode1!(
        vm::op_i64_gt_u,
        DecodedKind::Compare(TypedCompareOp::I64(IntCompareKind::GtU))
    );
    decode1!(
        vm::op_i64_le_s,
        DecodedKind::Compare(TypedCompareOp::I64(IntCompareKind::LeS))
    );
    decode1!(
        vm::op_i64_le_u,
        DecodedKind::Compare(TypedCompareOp::I64(IntCompareKind::LeU))
    );
    decode1!(
        vm::op_i64_ge_s,
        DecodedKind::Compare(TypedCompareOp::I64(IntCompareKind::GeS))
    );
    decode1!(
        vm::op_i64_ge_u,
        DecodedKind::Compare(TypedCompareOp::I64(IntCompareKind::GeU))
    );

    decode1!(
        vm::op_f32_eq,
        DecodedKind::Compare(TypedCompareOp::F32(FloatCompareKind::Eq))
    );
    decode1!(
        vm::op_f32_ne,
        DecodedKind::Compare(TypedCompareOp::F32(FloatCompareKind::Ne))
    );
    decode1!(
        vm::op_f32_lt,
        DecodedKind::Compare(TypedCompareOp::F32(FloatCompareKind::Lt))
    );
    decode1!(
        vm::op_f32_gt,
        DecodedKind::Compare(TypedCompareOp::F32(FloatCompareKind::Gt))
    );
    decode1!(
        vm::op_f32_le,
        DecodedKind::Compare(TypedCompareOp::F32(FloatCompareKind::Le))
    );
    decode1!(
        vm::op_f32_ge,
        DecodedKind::Compare(TypedCompareOp::F32(FloatCompareKind::Ge))
    );
    decode1!(
        vm::op_f64_eq,
        DecodedKind::Compare(TypedCompareOp::F64(FloatCompareKind::Eq))
    );
    decode1!(
        vm::op_f64_ne,
        DecodedKind::Compare(TypedCompareOp::F64(FloatCompareKind::Ne))
    );
    decode1!(
        vm::op_f64_lt,
        DecodedKind::Compare(TypedCompareOp::F64(FloatCompareKind::Lt))
    );
    decode1!(
        vm::op_f64_gt,
        DecodedKind::Compare(TypedCompareOp::F64(FloatCompareKind::Gt))
    );
    decode1!(
        vm::op_f64_le,
        DecodedKind::Compare(TypedCompareOp::F64(FloatCompareKind::Le))
    );
    decode1!(
        vm::op_f64_ge,
        DecodedKind::Compare(TypedCompareOp::F64(FloatCompareKind::Ge))
    );

    decode2!(
        vm::op_i32_load_local,
        DecodedKind::Load(TypedLoadOp::Bits4(Load4Kind::I32), unsafe {
            raw[1].operand.memarg
        })
    );
    decode2!(
        vm::op_i32_load8_s_local,
        DecodedKind::Load(TypedLoadOp::Bits4(Load4Kind::I32Load8S), unsafe {
            raw[1].operand.memarg
        })
    );
    decode2!(
        vm::op_i32_load8_u_local,
        DecodedKind::Load(TypedLoadOp::Bits4(Load4Kind::I32Load8U), unsafe {
            raw[1].operand.memarg
        })
    );
    decode2!(
        vm::op_i32_load16_s_local,
        DecodedKind::Load(TypedLoadOp::Bits4(Load4Kind::I32Load16S), unsafe {
            raw[1].operand.memarg
        })
    );
    decode2!(
        vm::op_i32_load16_u_local,
        DecodedKind::Load(TypedLoadOp::Bits4(Load4Kind::I32Load16U), unsafe {
            raw[1].operand.memarg
        })
    );
    decode2!(
        vm::op_i64_load_local,
        DecodedKind::Load(TypedLoadOp::Bits8(Load8Kind::I64), unsafe {
            raw[1].operand.memarg
        })
    );
    decode2!(
        vm::op_i64_load8_s_local,
        DecodedKind::Load(TypedLoadOp::Bits8(Load8Kind::I64Load8S), unsafe {
            raw[1].operand.memarg
        })
    );
    decode2!(
        vm::op_i64_load8_u_local,
        DecodedKind::Load(TypedLoadOp::Bits8(Load8Kind::I64Load8U), unsafe {
            raw[1].operand.memarg
        })
    );
    decode2!(
        vm::op_i64_load16_s_local,
        DecodedKind::Load(TypedLoadOp::Bits8(Load8Kind::I64Load16S), unsafe {
            raw[1].operand.memarg
        })
    );
    decode2!(
        vm::op_i64_load16_u_local,
        DecodedKind::Load(TypedLoadOp::Bits8(Load8Kind::I64Load16U), unsafe {
            raw[1].operand.memarg
        })
    );
    decode2!(
        vm::op_i64_load32_s_local,
        DecodedKind::Load(TypedLoadOp::Bits8(Load8Kind::I64Load32S), unsafe {
            raw[1].operand.memarg
        })
    );
    decode2!(
        vm::op_i64_load32_u_local,
        DecodedKind::Load(TypedLoadOp::Bits8(Load8Kind::I64Load32U), unsafe {
            raw[1].operand.memarg
        })
    );
    decode2!(
        vm::op_f32_load_local,
        DecodedKind::Load(TypedLoadOp::Bits4(Load4Kind::F32), unsafe {
            raw[1].operand.memarg
        })
    );
    decode2!(
        vm::op_f64_load_local,
        DecodedKind::Load(TypedLoadOp::Bits8(Load8Kind::F64), unsafe {
            raw[1].operand.memarg
        })
    );

    decode2!(
        vm::op_i32_store_local,
        DecodedKind::Store(TypedStoreOp::Bits4(Store4Kind::I32), unsafe {
            raw[1].operand.memarg
        })
    );
    decode2!(
        vm::op_i32_store8_local,
        DecodedKind::Store(TypedStoreOp::Bits4(Store4Kind::I32Store8), unsafe {
            raw[1].operand.memarg
        })
    );
    decode2!(
        vm::op_i32_store16_local,
        DecodedKind::Store(TypedStoreOp::Bits4(Store4Kind::I32Store16), unsafe {
            raw[1].operand.memarg
        })
    );
    decode2!(
        vm::op_i64_store_local,
        DecodedKind::Store(TypedStoreOp::Bits8(Store8Kind::I64), unsafe {
            raw[1].operand.memarg
        })
    );
    decode2!(
        vm::op_i64_store8_local,
        DecodedKind::Store(TypedStoreOp::Bits8(Store8Kind::I64Store8), unsafe {
            raw[1].operand.memarg
        })
    );
    decode2!(
        vm::op_i64_store16_local,
        DecodedKind::Store(TypedStoreOp::Bits8(Store8Kind::I64Store16), unsafe {
            raw[1].operand.memarg
        })
    );
    decode2!(
        vm::op_i64_store32_local,
        DecodedKind::Store(TypedStoreOp::Bits8(Store8Kind::I64Store32), unsafe {
            raw[1].operand.memarg
        })
    );
    decode2!(
        vm::op_f32_store_local,
        DecodedKind::Store(TypedStoreOp::Bits4(Store4Kind::F32), unsafe {
            raw[1].operand.memarg
        })
    );
    decode2!(
        vm::op_f64_store_local,
        DecodedKind::Store(TypedStoreOp::Bits8(Store8Kind::F64), unsafe {
            raw[1].operand.memarg
        })
    );

    DecodedKind::Raw
}
