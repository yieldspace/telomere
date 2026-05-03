use super::*;
use crate::common::{
    decode_local_binop32_kind, decode_local_binop64_kind, decode_local_cmp32_kind,
    decode_local_cmp64_kind, decode_local_unary32_kind, decode_local_unary64_kind, LocalBinop32Op,
    LocalBinop64Op, LocalCmp32Op, LocalCmp64Op, LocalFastConstKind, LocalFastRhsShape,
    LocalUnary32Op, LocalUnary64Op,
};

#[inline(always)]
fn local_i32(ctx: &mut ExecuteContext, addr: usize) -> i32 {
    unsafe {
        ctx.stack
            .local_u32_from_base(ctx.local_base_ptr as *const u8, addr) as i32
    }
}

#[inline(always)]
fn local_u32_bits(ctx: &mut ExecuteContext, addr: usize) -> u32 {
    unsafe {
        ctx.stack
            .local_u32_from_base(ctx.local_base_ptr as *const u8, addr)
    }
}

#[inline(always)]
fn local_u64_bits(ctx: &mut ExecuteContext, addr: usize) -> u64 {
    unsafe {
        u64::from_le(
            (ctx.local_base_ptr as *const u8)
                .add(addr)
                .cast::<u64>()
                .read_unaligned(),
        )
    }
}

#[inline(always)]
fn store_local8_bits(ctx: &mut ExecuteContext, addr: usize, value: u64) {
    unsafe {
        ctx.local_base_ptr
            .add(addr)
            .cast::<u64>()
            .write_unaligned(value.to_le());
    }
}

#[inline(always)]
fn local_fast_rhs_shape_name(shape: LocalFastRhsShape) -> &'static str {
    match shape {
        LocalFastRhsShape::Local => "local",
        LocalFastRhsShape::Const => "const",
    }
}

#[derive(Clone, Copy)]
struct LocalBinop32Descriptor {
    op: LocalBinop32Op,
    rhs_shape: LocalFastRhsShape,
    rhs_const_kind: LocalFastConstKind,
    supports_br_if: bool,
}

impl LocalBinop32Descriptor {
    #[inline(always)]
    fn decode(kind: u32) -> Option<Self> {
        let (op, rhs_shape) = decode_local_binop32_kind(kind)?;
        Some(Self {
            op,
            rhs_shape,
            rhs_const_kind: op.const_kind(),
            supports_br_if: matches!(
                op,
                LocalBinop32Op::I32Add
                    | LocalBinop32Op::I32Sub
                    | LocalBinop32Op::I32Mul
                    | LocalBinop32Op::I32And
                    | LocalBinop32Op::I32Or
                    | LocalBinop32Op::I32Xor
                    | LocalBinop32Op::I32Shl
                    | LocalBinop32Op::I32ShrS
                    | LocalBinop32Op::I32ShrU
                    | LocalBinop32Op::I32Rotl
                    | LocalBinop32Op::I32Rotr
            ),
        })
    }

    #[inline(always)]
    unsafe fn eval(self, tail_code: *const Instr, ctx: &mut ExecuteContext) -> u32 {
        if let Some(result) = self.fast_i32_eval(tail_code, ctx) {
            return result;
        }
        let lhs = unsafe { (*tail_code.add(1)).operand.local_addr as usize };
        let lhs_bits = local_u32_bits(ctx, lhs);
        let rhs_bits = load_binop32_rhs(tail_code, self.rhs_shape, self.rhs_const_kind, ctx);
        eval_local_binop32(self.op, lhs_bits, rhs_bits)
    }

    #[inline(always)]
    unsafe fn fast_i32_eval(
        self,
        tail_code: *const Instr,
        ctx: &mut ExecuteContext,
    ) -> Option<u32> {
        let lhs = unsafe { (*tail_code.add(1)).operand.local_addr as usize };
        let lhs_value = local_u32_bits(ctx, lhs);

        if self.op == LocalBinop32Op::I32Add && self.rhs_shape == LocalFastRhsShape::Local {
            let rhs = unsafe { (*tail_code.add(2)).operand.local_addr as usize };
            return Some(lhs_value.wrapping_add(local_u32_bits(ctx, rhs)));
        }
        if self.op == LocalBinop32Op::I32Add && self.rhs_shape == LocalFastRhsShape::Const {
            return Some(lhs_value.wrapping_add(unsafe { (*tail_code.add(2)).operand.i32 as u32 }));
        }
        if self.op == LocalBinop32Op::I32Sub && self.rhs_shape == LocalFastRhsShape::Local {
            let rhs = unsafe { (*tail_code.add(2)).operand.local_addr as usize };
            return Some(lhs_value.wrapping_sub(local_u32_bits(ctx, rhs)));
        }
        if self.op == LocalBinop32Op::I32Sub && self.rhs_shape == LocalFastRhsShape::Const {
            return Some(lhs_value.wrapping_sub(unsafe { (*tail_code.add(2)).operand.i32 as u32 }));
        }
        None
    }
}

#[derive(Clone, Copy)]
struct LocalBinop64Descriptor {
    op: LocalBinop64Op,
    rhs_shape: LocalFastRhsShape,
    rhs_const_kind: LocalFastConstKind,
}

impl LocalBinop64Descriptor {
    #[inline(always)]
    fn decode(kind: u32) -> Option<Self> {
        let (op, rhs_shape) = decode_local_binop64_kind(kind)?;
        Some(Self {
            op,
            rhs_shape,
            rhs_const_kind: op.const_kind(),
        })
    }

    #[inline(always)]
    unsafe fn eval(self, tail_code: *const Instr, ctx: &mut ExecuteContext) -> u64 {
        let lhs = unsafe { (*tail_code.add(1)).operand.local_addr as usize };
        let lhs_bits = local_u64_bits(ctx, lhs);
        let rhs_bits = load_binop64_rhs(tail_code, self.rhs_shape, self.rhs_const_kind, ctx);
        eval_local_binop64(self.op, lhs_bits, rhs_bits)
    }
}

#[derive(Clone, Copy)]
struct LocalCmp32Descriptor {
    op: LocalCmp32Op,
    rhs_shape: LocalFastRhsShape,
    rhs_const_kind: LocalFastConstKind,
}

impl LocalCmp32Descriptor {
    #[inline(always)]
    fn decode(kind: u32) -> Option<Self> {
        let (op, rhs_shape) = decode_local_cmp32_kind(kind)?;
        Some(Self {
            op,
            rhs_shape,
            rhs_const_kind: op.const_kind(),
        })
    }

    #[inline(always)]
    unsafe fn eval(self, tail_code: *const Instr, ctx: &mut ExecuteContext) -> u32 {
        let lhs = unsafe { (*tail_code.add(1)).operand.local_addr as usize };
        let lhs_bits = local_u32_bits(ctx, lhs);
        let rhs_bits = load_cmp32_rhs(tail_code, self.rhs_shape, self.rhs_const_kind, ctx);
        eval_local_cmp32(self.op, lhs_bits, rhs_bits)
    }
}

#[derive(Clone, Copy)]
struct LocalCmp64Descriptor {
    op: LocalCmp64Op,
    rhs_shape: LocalFastRhsShape,
    rhs_const_kind: LocalFastConstKind,
}

impl LocalCmp64Descriptor {
    #[inline(always)]
    fn decode(kind: u32) -> Option<Self> {
        let (op, rhs_shape) = decode_local_cmp64_kind(kind)?;
        Some(Self {
            op,
            rhs_shape,
            rhs_const_kind: op.const_kind(),
        })
    }

    #[inline(always)]
    unsafe fn eval(self, tail_code: *const Instr, ctx: &mut ExecuteContext) -> u32 {
        let lhs = unsafe { (*tail_code.add(1)).operand.local_addr as usize };
        let lhs_bits = local_u64_bits(ctx, lhs);
        let rhs_bits = load_cmp64_rhs(tail_code, self.rhs_shape, self.rhs_const_kind, ctx);
        eval_local_cmp64(self.op, lhs_bits, rhs_bits)
    }
}

#[derive(Clone, Copy)]
struct LocalUnary32Descriptor {
    op: LocalUnary32Op,
}

impl LocalUnary32Descriptor {
    #[inline(always)]
    fn decode(kind: u32) -> Option<Self> {
        Some(Self {
            op: decode_local_unary32_kind(kind)?,
        })
    }

    #[inline(always)]
    unsafe fn eval(self, tail_code: *const Instr, ctx: &mut ExecuteContext) -> u32 {
        let src = unsafe { (*tail_code.add(1)).operand.local_addr as usize };
        eval_local_unary32(self.op, local_u32_bits(ctx, src))
    }
}

#[derive(Clone, Copy)]
struct LocalUnary64Descriptor {
    op: LocalUnary64Op,
}

impl LocalUnary64Descriptor {
    #[inline(always)]
    fn decode(kind: u32) -> Option<Self> {
        Some(Self {
            op: decode_local_unary64_kind(kind)?,
        })
    }

    #[inline(always)]
    unsafe fn eval(self, tail_code: *const Instr, ctx: &mut ExecuteContext) -> u64 {
        let src = unsafe { (*tail_code.add(1)).operand.local_addr as usize };
        eval_local_unary64(self.op, local_u64_bits(ctx, src))
    }
}

#[inline(always)]
fn eval_local_unary32(op: LocalUnary32Op, src_bits: u32) -> u32 {
    match op {
        LocalUnary32Op::I32Clz => (src_bits as i32).leading_zeros(),
        LocalUnary32Op::I32Ctz => (src_bits as i32).trailing_zeros(),
        LocalUnary32Op::I32Popcnt => (src_bits as i32).count_ones(),
        LocalUnary32Op::F32Abs => f32::from_bits(src_bits).abs().to_bits(),
        LocalUnary32Op::F32Neg => (-f32::from_bits(src_bits)).to_bits(),
        LocalUnary32Op::F32Sqrt => f32::from_bits(src_bits).sqrt().to_bits(),
        LocalUnary32Op::F32Ceil => f32::from_bits(src_bits).ceil().to_bits(),
        LocalUnary32Op::F32Floor => f32::from_bits(src_bits).floor().to_bits(),
        LocalUnary32Op::F32Trunc => f32::from_bits(src_bits).trunc().to_bits(),
        LocalUnary32Op::F32Nearest => f32::from_bits(src_bits).round_ties_even().to_bits(),
    }
}

#[inline(always)]
fn eval_local_unary64(op: LocalUnary64Op, src_bits: u64) -> u64 {
    match op {
        LocalUnary64Op::I64Clz => (src_bits as i64).leading_zeros() as u64,
        LocalUnary64Op::I64Ctz => (src_bits as i64).trailing_zeros() as u64,
        LocalUnary64Op::I64Popcnt => (src_bits as i64).count_ones() as u64,
        LocalUnary64Op::F64Abs => f64::from_bits(src_bits).abs().to_bits(),
        LocalUnary64Op::F64Neg => (-f64::from_bits(src_bits)).to_bits(),
        LocalUnary64Op::F64Sqrt => f64::from_bits(src_bits).sqrt().to_bits(),
        LocalUnary64Op::F64Ceil => f64::from_bits(src_bits).ceil().to_bits(),
        LocalUnary64Op::F64Floor => f64::from_bits(src_bits).floor().to_bits(),
        LocalUnary64Op::F64Trunc => f64::from_bits(src_bits).trunc().to_bits(),
        LocalUnary64Op::F64Nearest => f64::from_bits(src_bits).round_ties_even().to_bits(),
    }
}

#[inline(always)]
fn eval_local_binop32(op: LocalBinop32Op, lhs_bits: u32, rhs_bits: u32) -> u32 {
    match op {
        LocalBinop32Op::I32Add => (lhs_bits as i32).wrapping_add(rhs_bits as i32) as u32,
        LocalBinop32Op::I32Sub => (lhs_bits as i32).wrapping_sub(rhs_bits as i32) as u32,
        LocalBinop32Op::I32Mul => (lhs_bits as i32).wrapping_mul(rhs_bits as i32) as u32,
        LocalBinop32Op::I32And => lhs_bits & rhs_bits,
        LocalBinop32Op::I32Or => lhs_bits | rhs_bits,
        LocalBinop32Op::I32Xor => lhs_bits ^ rhs_bits,
        LocalBinop32Op::I32Shl => wasm_i32_shl(lhs_bits as i32, rhs_bits as i32) as u32,
        LocalBinop32Op::I32ShrS => wasm_i32_shr_s(lhs_bits as i32, rhs_bits as i32) as u32,
        LocalBinop32Op::I32ShrU => wasm_i32_shr_u(lhs_bits, rhs_bits),
        LocalBinop32Op::I32Rotl => lhs_bits.rotate_left(rhs_bits),
        LocalBinop32Op::I32Rotr => lhs_bits.rotate_right(rhs_bits),
        LocalBinop32Op::F32Add => (f32::from_bits(lhs_bits) + f32::from_bits(rhs_bits)).to_bits(),
        LocalBinop32Op::F32Sub => (f32::from_bits(lhs_bits) - f32::from_bits(rhs_bits)).to_bits(),
        LocalBinop32Op::F32Mul => (f32::from_bits(lhs_bits) * f32::from_bits(rhs_bits)).to_bits(),
        LocalBinop32Op::F32Div => (f32::from_bits(lhs_bits) / f32::from_bits(rhs_bits)).to_bits(),
    }
}

#[inline(always)]
fn is_stack_i32_const_binop(op: LocalBinop32Op) -> bool {
    matches!(
        op,
        LocalBinop32Op::I32Add
            | LocalBinop32Op::I32Sub
            | LocalBinop32Op::I32Mul
            | LocalBinop32Op::I32And
            | LocalBinop32Op::I32Or
            | LocalBinop32Op::I32Xor
            | LocalBinop32Op::I32Shl
            | LocalBinop32Op::I32ShrS
            | LocalBinop32Op::I32ShrU
            | LocalBinop32Op::I32Rotl
            | LocalBinop32Op::I32Rotr
    )
}

#[inline(always)]
fn eval_stack_i32_const_binop(kind: u32, lhs_bits: u32, rhs_bits: u32) -> Option<u32> {
    let (op, rhs_shape) = decode_local_binop32_kind(kind)?;
    if rhs_shape != LocalFastRhsShape::Const || !is_stack_i32_const_binop(op) {
        return None;
    }
    Some(eval_local_binop32(op, lhs_bits, rhs_bits))
}

#[inline(always)]
fn is_stack_i32_const_cmp(op: LocalCmp32Op) -> bool {
    matches!(
        op,
        LocalCmp32Op::I32Eq
            | LocalCmp32Op::I32Ne
            | LocalCmp32Op::I32LtS
            | LocalCmp32Op::I32LtU
            | LocalCmp32Op::I32GtS
            | LocalCmp32Op::I32GtU
            | LocalCmp32Op::I32LeS
            | LocalCmp32Op::I32LeU
            | LocalCmp32Op::I32GeS
            | LocalCmp32Op::I32GeU
    )
}

#[inline(always)]
fn eval_stack_i32_const_cmp(kind: u32, lhs_bits: u32, rhs_bits: u32) -> Option<u32> {
    let (op, rhs_shape) = decode_local_cmp32_kind(kind)?;
    if rhs_shape != LocalFastRhsShape::Const || !is_stack_i32_const_cmp(op) {
        return None;
    }
    Some(eval_local_cmp32(op, lhs_bits, rhs_bits))
}

#[inline(always)]
fn eval_local_binop64(op: LocalBinop64Op, lhs_bits: u64, rhs_bits: u64) -> u64 {
    match op {
        LocalBinop64Op::I64Add => (lhs_bits as i64).wrapping_add(rhs_bits as i64) as u64,
        LocalBinop64Op::I64Sub => (lhs_bits as i64).wrapping_sub(rhs_bits as i64) as u64,
        LocalBinop64Op::I64Mul => (lhs_bits as i64).wrapping_mul(rhs_bits as i64) as u64,
        LocalBinop64Op::I64And => lhs_bits & rhs_bits,
        LocalBinop64Op::I64Or => lhs_bits | rhs_bits,
        LocalBinop64Op::I64Xor => lhs_bits ^ rhs_bits,
        LocalBinop64Op::I64Shl => wasm_i64_shl(lhs_bits as i64, rhs_bits as i64) as u64,
        LocalBinop64Op::I64ShrS => wasm_i64_shr_s(lhs_bits as i64, rhs_bits as i64) as u64,
        LocalBinop64Op::I64ShrU => wasm_i64_shr_u(lhs_bits, rhs_bits),
        LocalBinop64Op::I64Rotl => lhs_bits.rotate_left(rhs_bits as u32),
        LocalBinop64Op::I64Rotr => lhs_bits.rotate_right(rhs_bits as u32),
        LocalBinop64Op::F64Add => (f64::from_bits(lhs_bits) + f64::from_bits(rhs_bits)).to_bits(),
        LocalBinop64Op::F64Sub => (f64::from_bits(lhs_bits) - f64::from_bits(rhs_bits)).to_bits(),
        LocalBinop64Op::F64Mul => (f64::from_bits(lhs_bits) * f64::from_bits(rhs_bits)).to_bits(),
        LocalBinop64Op::F64Div => (f64::from_bits(lhs_bits) / f64::from_bits(rhs_bits)).to_bits(),
    }
}

#[inline(always)]
fn eval_local_cmp32(op: LocalCmp32Op, lhs_bits: u32, rhs_bits: u32) -> u32 {
    let cond = match op {
        LocalCmp32Op::I32Eq => lhs_bits == rhs_bits,
        LocalCmp32Op::I32Ne => lhs_bits != rhs_bits,
        LocalCmp32Op::I32LtS => (lhs_bits as i32) < (rhs_bits as i32),
        LocalCmp32Op::I32LtU => lhs_bits < rhs_bits,
        LocalCmp32Op::I32GtS => (lhs_bits as i32) > (rhs_bits as i32),
        LocalCmp32Op::I32GtU => lhs_bits > rhs_bits,
        LocalCmp32Op::I32LeS => (lhs_bits as i32) <= (rhs_bits as i32),
        LocalCmp32Op::I32LeU => lhs_bits <= rhs_bits,
        LocalCmp32Op::I32GeS => (lhs_bits as i32) >= (rhs_bits as i32),
        LocalCmp32Op::I32GeU => lhs_bits >= rhs_bits,
        LocalCmp32Op::F32Eq => f32::from_bits(lhs_bits) == f32::from_bits(rhs_bits),
        LocalCmp32Op::F32Ne => f32::from_bits(lhs_bits) != f32::from_bits(rhs_bits),
        LocalCmp32Op::F32Lt => f32::from_bits(lhs_bits) < f32::from_bits(rhs_bits),
        LocalCmp32Op::F32Gt => f32::from_bits(lhs_bits) > f32::from_bits(rhs_bits),
        LocalCmp32Op::F32Le => f32::from_bits(lhs_bits) <= f32::from_bits(rhs_bits),
        LocalCmp32Op::F32Ge => f32::from_bits(lhs_bits) >= f32::from_bits(rhs_bits),
    };
    cond as u32
}

#[inline(always)]
fn eval_local_cmp64(op: LocalCmp64Op, lhs_bits: u64, rhs_bits: u64) -> u32 {
    let cond = match op {
        LocalCmp64Op::I64Eq => lhs_bits == rhs_bits,
        LocalCmp64Op::I64Ne => lhs_bits != rhs_bits,
        LocalCmp64Op::I64LtS => (lhs_bits as i64) < (rhs_bits as i64),
        LocalCmp64Op::I64LtU => lhs_bits < rhs_bits,
        LocalCmp64Op::I64GtS => (lhs_bits as i64) > (rhs_bits as i64),
        LocalCmp64Op::I64GtU => lhs_bits > rhs_bits,
        LocalCmp64Op::I64LeS => (lhs_bits as i64) <= (rhs_bits as i64),
        LocalCmp64Op::I64LeU => lhs_bits <= rhs_bits,
        LocalCmp64Op::I64GeS => (lhs_bits as i64) >= (rhs_bits as i64),
        LocalCmp64Op::I64GeU => lhs_bits >= rhs_bits,
        LocalCmp64Op::F64Eq => f64::from_bits(lhs_bits) == f64::from_bits(rhs_bits),
        LocalCmp64Op::F64Ne => f64::from_bits(lhs_bits) != f64::from_bits(rhs_bits),
        LocalCmp64Op::F64Lt => f64::from_bits(lhs_bits) < f64::from_bits(rhs_bits),
        LocalCmp64Op::F64Gt => f64::from_bits(lhs_bits) > f64::from_bits(rhs_bits),
        LocalCmp64Op::F64Le => f64::from_bits(lhs_bits) <= f64::from_bits(rhs_bits),
        LocalCmp64Op::F64Ge => f64::from_bits(lhs_bits) >= f64::from_bits(rhs_bits),
    };
    cond as u32
}

#[inline(always)]
fn load_binop32_rhs(
    tail_code: *const Instr,
    shape: LocalFastRhsShape,
    const_kind: LocalFastConstKind,
    ctx: &mut ExecuteContext,
) -> u32 {
    match shape {
        LocalFastRhsShape::Local => {
            let rhs = unsafe { (*tail_code.add(2)).operand.local_addr as usize };
            local_u32_bits(ctx, rhs)
        }
        LocalFastRhsShape::Const => match const_kind {
            LocalFastConstKind::I32 => unsafe { (*tail_code.add(2)).operand.i32 as u32 },
            LocalFastConstKind::F32 => unsafe { (*tail_code.add(2)).operand.f32.to_bits() },
            _ => unreachable!("invalid 32-bit const kind"),
        },
    }
}

#[inline(always)]
fn load_binop64_rhs(
    tail_code: *const Instr,
    shape: LocalFastRhsShape,
    const_kind: LocalFastConstKind,
    ctx: &mut ExecuteContext,
) -> u64 {
    match shape {
        LocalFastRhsShape::Local => {
            let rhs = unsafe { (*tail_code.add(2)).operand.local_addr as usize };
            local_u64_bits(ctx, rhs)
        }
        LocalFastRhsShape::Const => match const_kind {
            LocalFastConstKind::I64 => unsafe { (*tail_code.add(2)).operand.i64 as u64 },
            LocalFastConstKind::F64 => unsafe { (*tail_code.add(2)).operand.f64.to_bits() },
            _ => unreachable!("invalid 64-bit const kind"),
        },
    }
}

#[inline(always)]
fn load_cmp32_rhs(
    tail_code: *const Instr,
    shape: LocalFastRhsShape,
    const_kind: LocalFastConstKind,
    ctx: &mut ExecuteContext,
) -> u32 {
    load_binop32_rhs(tail_code, shape, const_kind, ctx)
}

#[inline(always)]
fn load_cmp64_rhs(
    tail_code: *const Instr,
    shape: LocalFastRhsShape,
    const_kind: LocalFastConstKind,
    ctx: &mut ExecuteContext,
) -> u64 {
    load_binop64_rhs(tail_code, shape, const_kind, ctx)
}

#[inline(always)]
unsafe fn eval_binop32_from_tail(tail_code: *const Instr, ctx: &mut ExecuteContext) -> Option<u32> {
    let descriptor = LocalBinop32Descriptor::decode(unsafe { (*tail_code).operand.u32 })?;
    Some(unsafe { descriptor.eval(tail_code, ctx) })
}

#[inline(always)]
unsafe fn eval_i32_const_binop_from_tail(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Option<u32> {
    let kind = unsafe { (*tail_code).operand.u32 };
    let rhs = unsafe { (*tail_code.add(1)).operand.i32 as u32 };
    let lhs = ctx.stack.pop_u32();
    eval_stack_i32_const_binop(kind, lhs, rhs)
}

#[inline(always)]
unsafe fn eval_i32_const_cmp_from_tail(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> Option<u32> {
    let kind = unsafe { (*tail_code).operand.u32 };
    let rhs = unsafe { (*tail_code.add(1)).operand.i32 as u32 };
    let lhs = ctx.stack.pop_u32();
    eval_stack_i32_const_cmp(kind, lhs, rhs)
}

#[inline(always)]
unsafe fn eval_binop64_from_tail(tail_code: *const Instr, ctx: &mut ExecuteContext) -> Option<u64> {
    let descriptor = LocalBinop64Descriptor::decode(unsafe { (*tail_code).operand.u32 })?;
    Some(unsafe { descriptor.eval(tail_code, ctx) })
}

#[inline(always)]
unsafe fn eval_cmp32_from_tail(tail_code: *const Instr, ctx: &mut ExecuteContext) -> Option<u32> {
    let descriptor = LocalCmp32Descriptor::decode(unsafe { (*tail_code).operand.u32 })?;
    Some(unsafe { descriptor.eval(tail_code, ctx) })
}

#[inline(always)]
unsafe fn eval_cmp64_from_tail(tail_code: *const Instr, ctx: &mut ExecuteContext) -> Option<u32> {
    let descriptor = LocalCmp64Descriptor::decode(unsafe { (*tail_code).operand.u32 })?;
    Some(unsafe { descriptor.eval(tail_code, ctx) })
}

#[inline(always)]
unsafe fn eval_unary32_from_tail(tail_code: *const Instr, ctx: &mut ExecuteContext) -> Option<u32> {
    let descriptor = LocalUnary32Descriptor::decode(unsafe { (*tail_code).operand.u32 })?;
    Some(unsafe { descriptor.eval(tail_code, ctx) })
}

#[inline(always)]
unsafe fn eval_unary64_from_tail(tail_code: *const Instr, ctx: &mut ExecuteContext) -> Option<u64> {
    let descriptor = LocalUnary64Descriptor::decode(unsafe { (*tail_code).operand.u32 })?;
    Some(unsafe { descriptor.eval(tail_code, ctx) })
}

#[inline(always)]
fn i32_compare(kind: u32, lhs: i32, rhs: i32) -> bool {
    match kind {
        0 => lhs == rhs,
        1 => lhs != rhs,
        2 => lhs < rhs,
        3 => (lhs as u32) < (rhs as u32),
        4 => lhs > rhs,
        5 => (lhs as u32) > (rhs as u32),
        6 => lhs <= rhs,
        7 => (lhs as u32) <= (rhs as u32),
        8 => lhs >= rhs,
        9 => (lhs as u32) >= (rhs as u32),
        _ => false,
    }
}

#[inline(always)]
unsafe fn br_if_ptr(
    tail_code: *const Instr,
    target_offset: usize,
    taken_advance: usize,
    cond: u32,
    ctx: &mut ExecuteContext,
) -> *const Instr {
    if cond != 0 {
        let jump_addr = (*tail_code.add(target_offset)).operand.jump_addr;
        ctx.code().offset(jump_addr as isize)
    } else {
        tail_code.add(taken_advance)
    }
}

pub unsafe fn op_local_get4_i32_const_add(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let addr = (*tail_code).operand.local_addr as usize;
    let imm = (*tail_code.add(1)).operand.i32;
    let result = local_i32(ctx, addr).wrapping_add(imm);
    vm_try!(ctx.stack.push_i32(result));
    call_next(tail_code, 2, ctx)
}

pub unsafe fn op_local_get4_i32_const_add_set4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let src = (*tail_code).operand.local_addr as usize;
    let imm = (*tail_code.add(1)).operand.i32;
    let dst = (*tail_code.add(2)).operand.local_addr as usize;
    let result = local_i32(ctx, src).wrapping_add(imm) as u32;
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, dst, result);
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_local_get4_i32_const_add_tee4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let src = (*tail_code).operand.local_addr as usize;
    let imm = (*tail_code.add(1)).operand.i32;
    let dst = (*tail_code.add(2)).operand.local_addr as usize;
    let result = local_i32(ctx, src).wrapping_add(imm);
    vm_try!(ctx.stack.push_i32_fast(result));
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, dst, result as u32);
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_local_get4_set4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let src = (*tail_code).operand.local_addr as usize;
    let dst = (*tail_code.add(1)).operand.local_addr as usize;
    let value = local_u32_bits(ctx, src);
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, dst, value);
    call_next(tail_code, 2, ctx)
}

pub unsafe fn op_local_get4_tee4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let src = (*tail_code).operand.local_addr as usize;
    let dst = (*tail_code.add(1)).operand.local_addr as usize;
    let value = local_u32_bits(ctx, src);
    vm_try!(ctx.stack.push_u32_fast(value));
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, dst, value);
    call_next(tail_code, 2, ctx)
}

/// WebAssembly `local.get` run of two 32-bit locals.
///
/// Stack effect: `[] -> [i32, i32]`.
/// Traps: follows validated local addressing and stack-capacity invariants.
/// Notes: Generic predecoded superinstruction for consecutive `local.get` producers.
///
/// # Safety
/// - `tail_code` must point to operands decoded for this fused local handler.
/// - `ctx` must hold a valid frame and local base for the active function.
/// - This handler must not keep borrows, locks, or guards alive across `call_next`.
pub unsafe fn op_local_get4_local_get4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    dispatch_profile_count("op_local_get4_local_get4");
    let first = (*tail_code).operand.local_addr as usize;
    let second = (*tail_code.add(1)).operand.local_addr as usize;
    let first_value = local_u32_bits(ctx, first);
    let second_value = local_u32_bits(ctx, second);
    vm_try!(ctx.stack.push_u32_fast(first_value));
    vm_try!(ctx.stack.push_u32_fast(second_value));
    call_next(tail_code, 2, ctx)
}

/// WebAssembly `local.get` run of three 32-bit locals.
///
/// Stack effect: `[] -> [i32, i32, i32]`.
/// Traps: follows validated local addressing and stack-capacity invariants.
/// Notes: Generic predecoded superinstruction for consecutive `local.get` producers.
///
/// # Safety
/// - `tail_code` must point to operands decoded for this fused local handler.
/// - `ctx` must hold a valid frame and local base for the active function.
/// - This handler must not keep borrows, locks, or guards alive across `call_next`.
pub unsafe fn op_local_get4_local_get4_local_get4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    dispatch_profile_count("op_local_get4_local_get4_local_get4");
    let first = (*tail_code).operand.local_addr as usize;
    let second = (*tail_code.add(1)).operand.local_addr as usize;
    let third = (*tail_code.add(2)).operand.local_addr as usize;
    let first_value = local_u32_bits(ctx, first);
    let second_value = local_u32_bits(ctx, second);
    let third_value = local_u32_bits(ctx, third);
    vm_try!(ctx.stack.push_u32_fast(first_value));
    vm_try!(ctx.stack.push_u32_fast(second_value));
    vm_try!(ctx.stack.push_u32_fast(third_value));
    call_next(tail_code, 3, ctx)
}

/// WebAssembly `local.get` run of 32-bit locals.
///
/// Stack effect: `[] -> [i32 x count]`.
/// Traps: follows validated local addressing and stack-capacity invariants.
/// Notes: Generic run-length superinstruction for longer consecutive `local.get` producers.
///
/// # Safety
/// - `tail_code` must point to a count operand followed by that many local operands.
/// - `ctx` must hold a valid frame and local base for the active function.
/// - This handler must not keep borrows, locks, or guards alive across `call_next`.
pub unsafe fn op_local_get4_run(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    dispatch_profile_count("op_local_get4_run");
    let count = (*tail_code).operand.u32 as usize;
    debug_assert!((4..=8).contains(&count));
    for index in 0..count {
        let local = (*tail_code.add(1 + index)).operand.local_addr as usize;
        let value = local_u32_bits(ctx, local);
        vm_try!(ctx.stack.push_u32_fast(value));
    }
    call_next(
        tail_code,
        isize::try_from(1 + count).expect("local.get run width exceeds isize::MAX"),
        ctx,
    )
}

pub unsafe fn op_i32_const_set4(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let value = (*tail_code).operand.i32 as u32;
    let dst = (*tail_code.add(1)).operand.local_addr as usize;
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, dst, value);
    call_next(tail_code, 2, ctx)
}

pub unsafe fn op_i32_const_tee4(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let value = (*tail_code).operand.i32;
    let dst = (*tail_code.add(1)).operand.local_addr as usize;
    vm_try!(ctx.stack.push_i32_fast(value));
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, dst, value as u32);
    call_next(tail_code, 2, ctx)
}

pub unsafe fn op_local_get4_set4_local_get4_i32_const_compare_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let copy_src = (*tail_code).operand.local_addr as usize;
    let copy_dst = (*tail_code.add(1)).operand.local_addr as usize;
    let lhs = (*tail_code.add(2)).operand.local_addr as usize;
    let kind = (*tail_code.add(3)).operand.u32;
    let rhs = (*tail_code.add(4)).operand.i32;
    let value = local_u32_bits(ctx, copy_src);
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, copy_dst, value);
    let ptr = br_if_ptr(
        tail_code,
        5,
        6,
        i32_compare(kind, local_i32(ctx, lhs), rhs) as u32,
        ctx,
    );
    call_next(ptr, 0, ctx)
}

pub unsafe fn op_local_get4_local_get4_i32_add(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs = (*tail_code).operand.local_addr as usize;
    let rhs = (*tail_code.add(1)).operand.local_addr as usize;
    let result = local_i32(ctx, lhs).wrapping_add(local_i32(ctx, rhs));
    vm_try!(ctx.stack.push_i32(result));
    call_next(tail_code, 2, ctx)
}

pub unsafe fn op_local_get4_local_get4_i32_add_set4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs = (*tail_code).operand.local_addr as usize;
    let rhs = (*tail_code.add(1)).operand.local_addr as usize;
    let dst = (*tail_code.add(2)).operand.local_addr as usize;
    let result = local_i32(ctx, lhs).wrapping_add(local_i32(ctx, rhs)) as u32;
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, dst, result);
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_local_get4_local_get4_i32_add_tee4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs = (*tail_code).operand.local_addr as usize;
    let rhs = (*tail_code.add(1)).operand.local_addr as usize;
    let dst = (*tail_code.add(2)).operand.local_addr as usize;
    let result = local_i32(ctx, lhs).wrapping_add(local_i32(ctx, rhs));
    vm_try!(ctx.stack.push_i32_fast(result));
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, dst, result as u32);
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_local_get4_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let addr = (*tail_code).operand.local_addr as usize;
    let cond = ctx
        .stack
        .local_u32_from_base(ctx.local_base_ptr as *const u8, addr);
    let ptr = br_if_ptr(tail_code, 1, 2, cond, ctx);
    call_next(ptr, 0, ctx)
}

pub unsafe fn op_local_get4_i32_const_add_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let addr = (*tail_code).operand.local_addr as usize;
    let imm = (*tail_code.add(1)).operand.i32;
    let cond = local_i32(ctx, addr).wrapping_add(imm) as u32;
    let ptr = br_if_ptr(tail_code, 2, 3, cond, ctx);
    call_next(ptr, 0, ctx)
}

pub unsafe fn op_local_get4_local_get4_i32_add_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs = (*tail_code).operand.local_addr as usize;
    let rhs = (*tail_code.add(1)).operand.local_addr as usize;
    let cond = local_i32(ctx, lhs).wrapping_add(local_i32(ctx, rhs)) as u32;
    let ptr = br_if_ptr(tail_code, 2, 3, cond, ctx);
    call_next(ptr, 0, ctx)
}

pub unsafe fn op_local_get4_i32_eqz_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let addr = (*tail_code).operand.local_addr as usize;
    let cond = (local_i32(ctx, addr) == 0) as u32;
    let ptr = br_if_ptr(tail_code, 1, 2, cond, ctx);
    call_next(ptr, 0, ctx)
}

pub unsafe fn op_local_get4_i32_const_compare_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs = (*tail_code).operand.local_addr as usize;
    let kind = (*tail_code.add(1)).operand.u32;
    let rhs = (*tail_code.add(2)).operand.i32;
    let cond = i32_compare(kind, local_i32(ctx, lhs), rhs) as u32;
    let ptr = br_if_ptr(tail_code, 3, 4, cond, ctx);
    call_next(ptr, 0, ctx)
}

pub unsafe fn op_local_get4_local_get4_compare_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let lhs = (*tail_code).operand.local_addr as usize;
    let rhs = (*tail_code.add(1)).operand.local_addr as usize;
    let kind = (*tail_code.add(2)).operand.u32;
    let cond = i32_compare(kind, local_i32(ctx, lhs), local_i32(ctx, rhs)) as u32;
    let ptr = br_if_ptr(tail_code, 3, 4, cond, ctx);
    call_next(ptr, 0, ctx)
}

pub unsafe fn op_local_get4_i32_const_and_tee4_i32_const_eq_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let src = (*tail_code).operand.local_addr as usize;
    let mask = (*tail_code.add(1)).operand.i32 as u32;
    let dst = (*tail_code.add(2)).operand.local_addr as usize;
    let rhs = (*tail_code.add(3)).operand.i32 as u32;
    let value = local_u32_bits(ctx, src) & mask;
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, dst, value);
    let ptr = br_if_ptr(tail_code, 4, 5, (value == rhs) as u32, ctx);
    call_next(ptr, 0, ctx)
}

pub unsafe fn op_local_get4_i32_const_and_i32_const_compare_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let src = (*tail_code).operand.local_addr as usize;
    let mask = (*tail_code.add(1)).operand.i32 as u32;
    let kind = (*tail_code.add(2)).operand.u32;
    let rhs = (*tail_code.add(3)).operand.i32;
    let value = local_u32_bits(ctx, src) & mask;
    let ptr = br_if_ptr(
        tail_code,
        4,
        5,
        i32_compare(kind, value as i32, rhs) as u32,
        ctx,
    );
    call_next(ptr, 0, ctx)
}

pub unsafe fn op_local_get4_i32_const_add_i32_const_and_i32_const_compare_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let src = (*tail_code).operand.local_addr as usize;
    let imm = (*tail_code.add(1)).operand.i32 as u32;
    let mask = (*tail_code.add(2)).operand.i32 as u32;
    let kind = (*tail_code.add(3)).operand.u32;
    let rhs = (*tail_code.add(4)).operand.i32;
    let value = local_u32_bits(ctx, src).wrapping_add(imm) & mask;
    let ptr = br_if_ptr(
        tail_code,
        5,
        6,
        i32_compare(kind, value as i32, rhs) as u32,
        ctx,
    );
    call_next(ptr, 0, ctx)
}

pub unsafe fn op_local_get4_i32_const_and_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let src = (*tail_code).operand.local_addr as usize;
    let mask = (*tail_code.add(1)).operand.i32;
    let cond = (local_i32(ctx, src) & mask) as u32;
    let ptr = br_if_ptr(tail_code, 2, 3, cond, ctx);
    call_next(ptr, 0, ctx)
}

pub unsafe fn op_local_get4_i32_const_and_eqz_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let src = (*tail_code).operand.local_addr as usize;
    let mask = (*tail_code.add(1)).operand.i32;
    let cond = ((local_i32(ctx, src) & mask) == 0) as u32;
    let ptr = br_if_ptr(tail_code, 2, 3, cond, ctx);
    call_next(ptr, 0, ctx)
}

pub unsafe fn op_local_get4_i32_const_add_tee4_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let src = (*tail_code).operand.local_addr as usize;
    let imm = (*tail_code.add(1)).operand.i32;
    let dst = (*tail_code.add(2)).operand.local_addr as usize;
    let result = local_i32(ctx, src).wrapping_add(imm);
    let cond = result as u32;
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, dst, cond);
    let ptr = br_if_ptr(tail_code, 3, 4, cond, ctx);
    call_next(ptr, 0, ctx)
}

pub unsafe fn op_local_binop32(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let result = eval_binop32_from_tail(tail_code, ctx).expect("invalid local_binop32 kind");
    vm_try!(ctx.stack.push_u32_fast(result));
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_local_binop32_set4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let result = eval_binop32_from_tail(tail_code, ctx).expect("invalid local_binop32 kind");
    let dst = (*tail_code.add(3)).operand.local_addr as usize;
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, dst, result);
    call_next(tail_code, 4, ctx)
}

pub unsafe fn op_local_binop32_tee4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let result = eval_binop32_from_tail(tail_code, ctx).expect("invalid local_binop32 kind");
    let dst = (*tail_code.add(3)).operand.local_addr as usize;
    vm_try!(ctx.stack.push_u32_fast(result));
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, dst, result);
    call_next(tail_code, 4, ctx)
}

pub unsafe fn op_local_binop32_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let descriptor = LocalBinop32Descriptor::decode((*tail_code).operand.u32)
        .expect("invalid local_binop32 kind");
    debug_assert!(
        descriptor.supports_br_if,
        "unsupported br_if kind for {}",
        local_fast_rhs_shape_name(descriptor.rhs_shape)
    );
    let result = descriptor.eval(tail_code, ctx);
    let ptr = br_if_ptr(tail_code, 3, 4, result, ctx);
    call_next(ptr, 0, ctx)
}

pub unsafe fn op_i32_const_binop(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let result = eval_i32_const_binop_from_tail(tail_code, ctx).expect("invalid i32_const_binop");
    vm_try!(ctx.stack.push_u32_fast(result));
    call_next(tail_code, 2, ctx)
}

pub unsafe fn op_i32_const_binop_set4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let result =
        eval_i32_const_binop_from_tail(tail_code, ctx).expect("invalid i32_const_binop_set4");
    let dst = (*tail_code.add(2)).operand.local_addr as usize;
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, dst, result);
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_i32_const_binop_tee4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let result =
        eval_i32_const_binop_from_tail(tail_code, ctx).expect("invalid i32_const_binop_tee4");
    let dst = (*tail_code.add(2)).operand.local_addr as usize;
    vm_try!(ctx.stack.push_u32_fast(result));
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, dst, result);
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_i32_const_binop_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let result =
        eval_i32_const_binop_from_tail(tail_code, ctx).expect("invalid i32_const_binop_br_if");
    let ptr = br_if_ptr(tail_code, 2, 3, result, ctx);
    call_next(ptr, 0, ctx)
}

pub unsafe fn op_i32_const_cmp(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let result = eval_i32_const_cmp_from_tail(tail_code, ctx).expect("invalid i32_const_cmp");
    vm_try!(ctx.stack.push_u32_fast(result));
    call_next(tail_code, 2, ctx)
}

pub unsafe fn op_i32_const_cmp_set4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let result = eval_i32_const_cmp_from_tail(tail_code, ctx).expect("invalid i32_const_cmp_set4");
    let dst = (*tail_code.add(2)).operand.local_addr as usize;
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, dst, result);
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_i32_const_cmp_tee4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let result = eval_i32_const_cmp_from_tail(tail_code, ctx).expect("invalid i32_const_cmp_tee4");
    let dst = (*tail_code.add(2)).operand.local_addr as usize;
    vm_try!(ctx.stack.push_u32_fast(result));
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, dst, result);
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_i32_const_cmp_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let result = eval_i32_const_cmp_from_tail(tail_code, ctx).expect("invalid i32_const_cmp_br_if");
    let ptr = br_if_ptr(tail_code, 2, 3, result, ctx);
    call_next(ptr, 0, ctx)
}

pub unsafe fn op_local_binop64(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let result = eval_binop64_from_tail(tail_code, ctx).expect("invalid local_binop64 kind");
    vm_try!(ctx.stack.push_u64_fast(result));
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_local_binop64_set8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let result = eval_binop64_from_tail(tail_code, ctx).expect("invalid local_binop64 kind");
    let dst = (*tail_code.add(3)).operand.local_addr as usize;
    store_local8_bits(ctx, dst, result);
    call_next(tail_code, 4, ctx)
}

pub unsafe fn op_local_binop64_tee8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let result = eval_binop64_from_tail(tail_code, ctx).expect("invalid local_binop64 kind");
    let dst = (*tail_code.add(3)).operand.local_addr as usize;
    vm_try!(ctx.stack.push_u64_fast(result));
    store_local8_bits(ctx, dst, result);
    call_next(tail_code, 4, ctx)
}

pub unsafe fn op_local_cmp32(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let result = eval_cmp32_from_tail(tail_code, ctx).expect("invalid local_cmp32 kind");
    vm_try!(ctx.stack.push_u32_fast(result));
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_local_cmp32_set4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let result = eval_cmp32_from_tail(tail_code, ctx).expect("invalid local_cmp32 kind");
    let dst = (*tail_code.add(3)).operand.local_addr as usize;
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, dst, result);
    call_next(tail_code, 4, ctx)
}

pub unsafe fn op_local_cmp32_tee4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let result = eval_cmp32_from_tail(tail_code, ctx).expect("invalid local_cmp32 kind");
    let dst = (*tail_code.add(3)).operand.local_addr as usize;
    vm_try!(ctx.stack.push_u32_fast(result));
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, dst, result);
    call_next(tail_code, 4, ctx)
}

pub unsafe fn op_local_cmp32_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let result = eval_cmp32_from_tail(tail_code, ctx).expect("invalid local_cmp32 kind");
    let ptr = br_if_ptr(tail_code, 3, 4, result, ctx);
    call_next(ptr, 0, ctx)
}

pub unsafe fn op_local_cmp64(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let result = eval_cmp64_from_tail(tail_code, ctx).expect("invalid local_cmp64 kind");
    vm_try!(ctx.stack.push_u32_fast(result));
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_local_cmp64_set4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let result = eval_cmp64_from_tail(tail_code, ctx).expect("invalid local_cmp64 kind");
    let dst = (*tail_code.add(3)).operand.local_addr as usize;
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, dst, result);
    call_next(tail_code, 4, ctx)
}

pub unsafe fn op_local_cmp64_tee4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let result = eval_cmp64_from_tail(tail_code, ctx).expect("invalid local_cmp64 kind");
    let dst = (*tail_code.add(3)).operand.local_addr as usize;
    vm_try!(ctx.stack.push_u32_fast(result));
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, dst, result);
    call_next(tail_code, 4, ctx)
}

pub unsafe fn op_local_cmp64_br_if(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let result = eval_cmp64_from_tail(tail_code, ctx).expect("invalid local_cmp64 kind");
    let ptr = br_if_ptr(tail_code, 3, 4, result, ctx);
    call_next(ptr, 0, ctx)
}

pub unsafe fn op_local_unary32(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let result = eval_unary32_from_tail(tail_code, ctx).expect("invalid local_unary32 kind");
    vm_try!(ctx.stack.push_u32_fast(result));
    call_next(tail_code, 2, ctx)
}

pub unsafe fn op_local_unary32_set4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let result = eval_unary32_from_tail(tail_code, ctx).expect("invalid local_unary32 kind");
    let dst = (*tail_code.add(2)).operand.local_addr as usize;
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, dst, result);
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_local_unary32_tee4(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let result = eval_unary32_from_tail(tail_code, ctx).expect("invalid local_unary32 kind");
    let dst = (*tail_code.add(2)).operand.local_addr as usize;
    vm_try!(ctx.stack.push_u32_fast(result));
    ctx.stack
        .local_set4_from_base_value(ctx.local_base_ptr, dst, result);
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_local_unary64(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    let result = eval_unary64_from_tail(tail_code, ctx).expect("invalid local_unary64 kind");
    vm_try!(ctx.stack.push_u64_fast(result));
    call_next(tail_code, 2, ctx)
}

pub unsafe fn op_local_unary64_set8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let result = eval_unary64_from_tail(tail_code, ctx).expect("invalid local_unary64 kind");
    let dst = (*tail_code.add(2)).operand.local_addr as usize;
    store_local8_bits(ctx, dst, result);
    call_next(tail_code, 3, ctx)
}

pub unsafe fn op_local_unary64_tee8(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    let result = eval_unary64_from_tail(tail_code, ctx).expect("invalid local_unary64 kind");
    let dst = (*tail_code.add(2)).operand.local_addr as usize;
    vm_try!(ctx.stack.push_u64_fast(result));
    store_local8_bits(ctx, dst, result);
    call_next(tail_code, 3, ctx)
}
