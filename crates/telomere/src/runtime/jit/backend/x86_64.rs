//! x86_64 target hooks for the shared baseline lowering.
//!
//! x86_64 diverges from the defaults in three places:
//!
//! - `rem_s`: the virtual `sdiv` is a real `idiv`, which raises #DE
//!   on INT_MIN / -1 (wasm `rem_s` must yield 0 there), so the
//!   remainder sequence needs an explicit overflow guard.
//! - trunc-sat: computed by a runtime stub instead of a native
//!   sequence (the same stub shapes riscv64 uses).
//! - the generic float-unary rounding sites: their historical byte
//!   stream carries extra `fmov`s around the rounding sandwich
//!   (the prologue move was not elided when the rounding helper was
//!   factored out), and the unary hooks reproduce it exactly.

use telomere_jit_codegen::arch::x86_64::X64BaselineMasm;
use telomere_jit_codegen::masm::Cond;

use super::lowering::{patch_branch, Emitter, FixupKind, TargetHooks};
use super::ops::FloatUnaryOp;
use crate::runtime::jit::emit_env::EmitEnv;
use crate::runtime::jit::stubs::StubId;

impl TargetHooks for X64BaselineMasm {
    fn emit_i32_rem_s<'a, E: EmitEnv>(
        emitter: &mut Emitter<'a, E, Self>,
        lhs: u8,
        rhs: u8,
    ) -> Result<(), ()> {
        emitter.mov_imm_u32(17, 0x8000_0000);
        emitter.cmp_w(lhs, 17);
        let lhs_ok_branch = emitter.branch_placeholder(FixupKind::BCond(Cond::Ne));
        emitter.cmp_w_u32(rhs, u32::MAX);
        let rhs_ok_branch = emitter.branch_placeholder(FixupKind::BCond(Cond::Ne));
        emitter.mov_imm_u32(lhs, 0);
        let done_branch = emitter.branch_placeholder(FixupKind::B);

        let normal_target = emitter.offset();
        patch_branch::<Self>(
            emitter.as_mut_bytes(),
            lhs_ok_branch,
            normal_target,
            FixupKind::BCond(Cond::Ne),
        )?;
        patch_branch::<Self>(
            emitter.as_mut_bytes(),
            rhs_ok_branch,
            normal_target,
            FixupKind::BCond(Cond::Ne),
        )?;
        emitter.sdiv_w(17, lhs, rhs);
        emitter.msub_w(lhs, 17, rhs, lhs);

        let done_target = emitter.offset();
        patch_branch::<Self>(
            emitter.as_mut_bytes(),
            done_branch,
            done_target,
            FixupKind::B,
        )?;
        Ok(())
    }

    fn emit_i64_rem_s<'a, E: EmitEnv>(
        emitter: &mut Emitter<'a, E, Self>,
        lhs: u8,
        rhs: u8,
    ) -> Result<(), ()> {
        emitter.mov_imm_u64(9, 0x8000_0000_0000_0000);
        emitter.cmp_x(lhs, 9);
        let lhs_ok_branch = emitter.branch_placeholder(FixupKind::BCond(Cond::Ne));
        emitter.mov_imm_u64(9, u64::MAX);
        emitter.cmp_x(rhs, 9);
        let rhs_ok_branch = emitter.branch_placeholder(FixupKind::BCond(Cond::Ne));
        emitter.mov_imm_u64(lhs, 0);
        let done_branch = emitter.branch_placeholder(FixupKind::B);

        let normal_target = emitter.offset();
        patch_branch::<Self>(
            emitter.as_mut_bytes(),
            lhs_ok_branch,
            normal_target,
            FixupKind::BCond(Cond::Ne),
        )?;
        patch_branch::<Self>(
            emitter.as_mut_bytes(),
            rhs_ok_branch,
            normal_target,
            FixupKind::BCond(Cond::Ne),
        )?;
        emitter.sdiv_x(9, lhs, rhs);
        emitter.msub_x(lhs, 9, rhs, lhs);

        let done_target = emitter.offset();
        patch_branch::<Self>(
            emitter.as_mut_bytes(),
            done_branch,
            done_target,
            FixupKind::B,
        )?;
        Ok(())
    }

    fn emit_f32_rounding_unary<'a, E: EmitEnv>(
        emitter: &mut Emitter<'a, E, Self>,
        value: u8,
        op: FloatUnaryOp,
    ) -> Result<(), ()> {
        // The historical emission at this site moves the value into
        // the float register file before the rounding helper does it
        // again; reproduce it byte for byte.
        emitter.fmov_s_from_w(0, value);
        emitter.emit_f32_rounding(value, op)
    }

    fn emit_f64_rounding_unary<'a, E: EmitEnv>(
        emitter: &mut Emitter<'a, E, Self>,
        value: u8,
        op: FloatUnaryOp,
    ) -> Result<(), ()> {
        // Same historical double move as the f32 site, plus the move
        // back after the helper (the shared emitter emits neither).
        emitter.fmov_d_from_x(0, value);
        emitter.emit_f64_rounding(value, op)?;
        emitter.fmov_x_from_d(value, 0);
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
