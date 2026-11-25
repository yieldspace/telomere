//! riscv64 target hooks for the shared baseline lowering.
//!
//! riscv64 uses the default remainder sequence (its virtual `sdiv`
//! sequence cannot trap on INT_MIN / -1) but its encoder has no
//! native float-rounding or trunc-sat sequences, so both go through
//! runtime stubs (the trunc-sat stub shapes are the same ones
//! x86_64 uses).

use telomere_jit_codegen::arch::riscv64::Riscv64BaselineMasm;

use super::lowering::{Emitter, TargetHooks};
use super::ops::FloatUnaryOp;
use crate::runtime::jit::emit_env::EmitEnv;
use crate::runtime::jit::stubs::StubId;

impl TargetHooks for Riscv64BaselineMasm {
    fn emit_f32_rounding<'a, E: EmitEnv>(
        emitter: &mut Emitter<'a, E, Self>,
        value: u8,
        op: FloatUnaryOp,
    ) -> Result<(), ()> {
        let helper = match op {
            FloatUnaryOp::Ceil => StubId::F32CeilBits,
            FloatUnaryOp::Floor => StubId::F32FloorBits,
            FloatUnaryOp::Trunc => StubId::F32TruncBits,
            FloatUnaryOp::Nearest => StubId::F32NearestBits,
            FloatUnaryOp::Abs | FloatUnaryOp::Neg | FloatUnaryOp::Sqrt => return Err(()),
        };
        emitter.mov_w(0, value);
        emitter.call_stub(helper);
        emitter.mov_w(value, 0);
        Ok(())
    }

    fn emit_f64_rounding<'a, E: EmitEnv>(
        emitter: &mut Emitter<'a, E, Self>,
        value: u8,
        op: FloatUnaryOp,
    ) -> Result<(), ()> {
        let helper = match op {
            FloatUnaryOp::Ceil => StubId::F64CeilBits,
            FloatUnaryOp::Floor => StubId::F64FloorBits,
            FloatUnaryOp::Trunc => StubId::F64TruncBits,
            FloatUnaryOp::Nearest => StubId::F64NearestBits,
            FloatUnaryOp::Abs | FloatUnaryOp::Neg | FloatUnaryOp::Sqrt => return Err(()),
        };
        emitter.mov_x(0, value);
        emitter.call_stub(helper);
        emitter.mov_x(value, 0);
        Ok(())
    }

    fn emit_i32_trunc_sat_f32<'a, E: EmitEnv>(
        emitter: &mut Emitter<'a, E, Self>,
        value: u8,
        signed: bool,
    ) -> Result<(), ()> {
        emitter.mov_w(0, value);
        emitter.mov_imm_u32(1, u32::from(signed));
        emitter.call_stub(StubId::I32TruncSatF32);
        emitter.return_if_exit();
        let result = emitter.push_reg()?;
        emitter.mov_w(result, 1);
        Ok(())
    }

    fn emit_i32_trunc_sat_f64<'a, E: EmitEnv>(
        emitter: &mut Emitter<'a, E, Self>,
        low: u8,
        high: u8,
        signed: bool,
    ) -> Result<(), ()> {
        emitter.pack_i64_slots_to_x(0, low, high, 9)?;
        emitter.mov_imm_u32(1, u32::from(signed));
        emitter.call_stub(StubId::I32TruncSatF64);
        emitter.return_if_exit();
        let result = emitter.push_reg()?;
        emitter.mov_w(result, 1);
        Ok(())
    }
}
