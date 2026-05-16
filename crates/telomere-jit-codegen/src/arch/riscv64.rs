use crate::{
    masm::{AsmError, AsmResult},
    target::{TargetArch, TargetInfo, TargetOs},
};

const VREGS: usize = 32;
const SLOT_SIZE: i32 = 8;
const FRAME_SIZE: i32 = ((VREGS as i32 * SLOT_SIZE + 16 + 15) & !15) + 16;

const ZERO: u8 = 0;
const RA: u8 = 1;
const SP: u8 = 2;
const T0: u8 = 5;
const T1: u8 = 6;
const T2: u8 = 7;
const S0: u8 = 8;
const A0: u8 = 10;
const A1: u8 = 11;
const A2: u8 = 12;
const A3: u8 = 13;
const A4: u8 = 14;
const A5: u8 = 15;
const A6: u8 = 16;
const A7: u8 = 17;
const T3: u8 = 28;
const T4: u8 = 29;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cond {
    Eq,
    Ne,
    Hs,
    Lo,
    Hi,
    Ls,
    Ge,
    Lt,
    Gt,
    Le,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BranchKind {
    B,
    BCond(Cond),
    CbnzX(u8),
    CbnzW(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Width {
    W32,
    X64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FloatWidth {
    F32,
    F64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Rhs {
    Reg(u8),
    Imm32(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LastCmp {
    Int { width: Width, lhs: u8, rhs: Rhs },
    Float { width: FloatWidth, lhs: u8, rhs: u8 },
}

impl Cond {
    pub const fn inverted(self) -> Self {
        match self {
            Self::Eq => Self::Ne,
            Self::Ne => Self::Eq,
            Self::Lo => Self::Hs,
            Self::Hs => Self::Lo,
            Self::Hi => Self::Ls,
            Self::Ls => Self::Hi,
            Self::Lt => Self::Ge,
            Self::Ge => Self::Lt,
            Self::Gt => Self::Le,
            Self::Le => Self::Gt,
        }
    }
}

pub fn target_info() -> TargetInfo {
    TargetInfo {
        arch: TargetArch::Riscv64,
        os: target_os(),
        baseline_supported: cfg!(all(
            target_os = "linux",
            target_arch = "riscv64",
            target_env = "gnu"
        )),
    }
}

const fn target_os() -> TargetOs {
    #[cfg(target_os = "linux")]
    {
        TargetOs::Linux
    }
    #[cfg(not(target_os = "linux"))]
    {
        TargetOs::Unsupported
    }
}

pub fn patch_branch(bytes: &mut [u8], at: usize, target: usize, _kind: BranchKind) -> AsmResult {
    let delta = target as isize - at as isize;
    if delta % 2 != 0 {
        return Err(AsmError::InvalidImmediate);
    }
    let insn = encode_jal(ZERO, delta).ok_or(AsmError::BranchOutOfRange)?;
    bytes
        .get_mut(at..at + 4)
        .ok_or(AsmError::InvalidImmediate)?
        .copy_from_slice(&insn.to_le_bytes());
    Ok(())
}

#[derive(Debug, Clone)]
pub struct Riscv64BaselineMasm {
    bytes: Vec<u8>,
    prologue_emitted: bool,
    epilogue_emitted: bool,
    pending_rbit: Option<(bool, u8, u8)>,
    last_cmp: Option<LastCmp>,
}

impl Riscv64BaselineMasm {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            bytes: Vec::with_capacity(capacity),
            prologue_emitted: false,
            epilogue_emitted: false,
            pending_rbit: None,
            last_cmp: None,
        }
    }

    pub fn offset(&self) -> usize {
        self.bytes.len()
    }

    pub fn truncate(&mut self, len: usize) {
        self.bytes.truncate(len);
    }

    pub fn as_mut_bytes(&mut self) -> &mut [u8] {
        &mut self.bytes
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    pub fn branch_placeholder(&mut self, kind: BranchKind) -> AsmResult<usize> {
        match kind {
            BranchKind::B => {
                let at = self.offset();
                self.insn(encode_jal(ZERO, 0).expect("zero jal encodes"));
                Ok(at)
            }
            BranchKind::BCond(cond) => {
                let cmp = self.last_cmp.ok_or(AsmError::InvalidImmediate)?;
                self.emit_cmp_branch_to_skip(cmp, cond.inverted())?;
                let at = self.offset();
                self.insn(encode_jal(ZERO, 0).expect("zero jal encodes"));
                Ok(at)
            }
            BranchKind::CbnzX(reg) => {
                self.load_slot64(T0, reg)?;
                self.insn(encode_b(0b000, T0, ZERO, 8).expect("local branch fits"));
                let at = self.offset();
                self.insn(encode_jal(ZERO, 0).expect("zero jal encodes"));
                Ok(at)
            }
            BranchKind::CbnzW(reg) => {
                self.load_slot32(T0, reg)?;
                self.insn(encode_b(0b000, T0, ZERO, 8).expect("local branch fits"));
                let at = self.offset();
                self.insn(encode_jal(ZERO, 0).expect("zero jal encodes"));
                Ok(at)
            }
        }
    }

    pub fn ret(&mut self) {
        self.insn(encode_i(0x67, 0, ZERO, RA, 0).expect("ret encodes"));
    }

    pub fn mov_x_from_sp(&mut self, rd: u8) {
        let _ = rd;
    }

    pub fn blr_x(&mut self, rn: u8) {
        self.load_slot64(T0, rn).expect("virtual register fits");
        for (slot, reg) in [A0, A1, A2, A3, A4, A5, A6, A7].into_iter().enumerate() {
            self.load_slot64(reg, slot as u8)
                .expect("argument slot fits");
        }
        self.insn(encode_i(0x67, 0, RA, T0, 0).expect("jalr encodes"));
        self.store_slot64(0, A0).expect("return slot fits");
        self.store_slot64(1, A1).expect("return slot fits");
    }

    pub fn mov_x(&mut self, rd: u8, rn: u8) {
        self.load_slot64(T0, rn).expect("virtual register fits");
        self.store_slot64(rd, T0).expect("virtual register fits");
    }

    pub fn mov_w(&mut self, rd: u8, rn: u8) {
        self.load_slot32(T0, rn).expect("virtual register fits");
        self.store_slot32(rd, T0).expect("virtual register fits");
    }

    pub fn mov_imm_u32(&mut self, rd: u8, value: u32) {
        self.li(T0, i64::from(value as i32));
        self.store_slot32(rd, T0).expect("virtual register fits");
    }

    pub fn mov_imm_u64(&mut self, rd: u8, value: u64) {
        self.li(T0, value as i64);
        self.store_slot64(rd, T0).expect("virtual register fits");
    }

    pub fn stp_pre_x_sp(&mut self, rt: u8, rt2: u8) {
        self.stp_pre(rt, rt2);
    }

    pub fn stp_pre(&mut self, rt: u8, rt2: u8) {
        if !self.prologue_emitted && rt == 29 && rt2 == 30 {
            self.prologue_emitted = true;
            self.addi(SP, SP, -FRAME_SIZE).expect("frame fits");
            self.store_mem64(SP, FRAME_SIZE - 8, RA)
                .expect("ra spill fits");
            self.store_mem64(SP, FRAME_SIZE - 16, S0)
                .expect("s0 spill fits");
            self.addi(S0, SP, FRAME_SIZE).expect("frame fits");
            for (slot, reg) in [A0, A1, A2, A3, A4, A5, A6, A7].into_iter().enumerate() {
                self.store_slot64(slot as u8, reg)
                    .expect("entry argument slot fits");
            }
        }
    }

    pub fn ldp_post_x_sp(&mut self, rt: u8, rt2: u8) {
        self.ldp_post(rt, rt2);
    }

    pub fn ldp_post(&mut self, rt: u8, rt2: u8) {
        if !self.epilogue_emitted && rt == 29 && rt2 == 30 {
            self.epilogue_emitted = true;
            self.load_slot64(A0, 0).expect("return slot fits");
            self.load_slot64(A1, 1).expect("return slot fits");
            self.load_mem64(S0, SP, FRAME_SIZE - 16)
                .expect("s0 reload fits");
            self.load_mem64(RA, SP, FRAME_SIZE - 8)
                .expect("ra reload fits");
            self.addi(SP, SP, FRAME_SIZE).expect("frame fits");
        }
    }

    pub fn add_imm_u32(&mut self, rd: u8, rn: u8, imm: u32) -> AsmResult {
        self.load_slot32(T0, rn)?;
        self.li(T1, i64::from(imm as i32));
        self.insn(encode_r(0x3b, 0, 0, T0, T0, T1));
        self.store_slot32(rd, T0)
    }

    pub fn add_imm_u64(&mut self, rd: u8, rn: u8, imm: u64) -> AsmResult {
        self.load_slot64(T0, rn)?;
        self.li(T1, imm as i64);
        self.insn(encode_r(0x33, 0, 0, T0, T0, T1));
        self.store_slot64(rd, T0)
    }

    pub fn sub_imm_u32(&mut self, rd: u8, rn: u8, imm: u32) -> AsmResult {
        self.load_slot32(T0, rn)?;
        self.li(T1, i64::from(imm as i32));
        self.insn(encode_r(0x3b, 0, 0x20, T0, T0, T1));
        self.store_slot32(rd, T0)
    }

    pub fn add_w(&mut self, rd: u8, rn: u8, rm: u8) {
        self.binop(rd, rn, rm, Width::W32, 0, 0);
    }

    pub fn add_x(&mut self, rd: u8, rn: u8, rm: u8) {
        self.binop(rd, rn, rm, Width::X64, 0, 0);
    }

    pub fn sub_w(&mut self, rd: u8, rn: u8, rm: u8) {
        self.binop(rd, rn, rm, Width::W32, 0, 0x20);
    }

    pub fn sub_x(&mut self, rd: u8, rn: u8, rm: u8) {
        self.binop(rd, rn, rm, Width::X64, 0, 0x20);
    }

    pub fn mul_w(&mut self, rd: u8, rn: u8, rm: u8) {
        self.binop(rd, rn, rm, Width::W32, 0, 0x01);
    }

    pub fn mul_x(&mut self, rd: u8, rn: u8, rm: u8) {
        self.binop(rd, rn, rm, Width::X64, 0, 0x01);
    }

    pub fn udiv_w(&mut self, rd: u8, rn: u8, rm: u8) {
        self.binop(rd, rn, rm, Width::W32, 5, 0x01);
    }

    pub fn sdiv_w(&mut self, rd: u8, rn: u8, rm: u8) {
        self.binop(rd, rn, rm, Width::W32, 4, 0x01);
    }

    pub fn udiv_x(&mut self, rd: u8, rn: u8, rm: u8) {
        self.binop(rd, rn, rm, Width::X64, 5, 0x01);
    }

    pub fn sdiv_x(&mut self, rd: u8, rn: u8, rm: u8) {
        self.binop(rd, rn, rm, Width::X64, 4, 0x01);
    }

    pub fn msub_w(&mut self, rd: u8, rn: u8, rm: u8, ra: u8) {
        self.load_slot32(T0, rn).expect("virtual register fits");
        self.load_slot32(T1, rm).expect("virtual register fits");
        self.insn(encode_r(0x3b, 0, 0x01, T0, T0, T1));
        self.load_slot32(T1, ra).expect("virtual register fits");
        self.insn(encode_r(0x3b, 0, 0x20, T0, T1, T0));
        self.store_slot32(rd, T0).expect("virtual register fits");
    }

    pub fn msub_x(&mut self, rd: u8, rn: u8, rm: u8, ra: u8) {
        self.load_slot64(T0, rn).expect("virtual register fits");
        self.load_slot64(T1, rm).expect("virtual register fits");
        self.insn(encode_r(0x33, 0, 0x01, T0, T0, T1));
        self.load_slot64(T1, ra).expect("virtual register fits");
        self.insn(encode_r(0x33, 0, 0x20, T0, T1, T0));
        self.store_slot64(rd, T0).expect("virtual register fits");
    }

    pub fn neg_w(&mut self, rd: u8, rm: u8) {
        self.load_slot32(T0, rm).expect("virtual register fits");
        self.insn(encode_r(0x3b, 0, 0x20, T0, ZERO, T0));
        self.store_slot32(rd, T0).expect("virtual register fits");
    }

    pub fn neg_x(&mut self, rd: u8, rm: u8) {
        self.load_slot64(T0, rm).expect("virtual register fits");
        self.insn(encode_r(0x33, 0, 0x20, T0, ZERO, T0));
        self.store_slot64(rd, T0).expect("virtual register fits");
    }

    pub fn and_w(&mut self, rd: u8, rn: u8, rm: u8) {
        self.binop(rd, rn, rm, Width::W32, 7, 0);
    }

    pub fn and_x(&mut self, rd: u8, rn: u8, rm: u8) {
        self.binop(rd, rn, rm, Width::X64, 7, 0);
    }

    pub fn orr_w(&mut self, rd: u8, rn: u8, rm: u8) {
        self.binop(rd, rn, rm, Width::W32, 6, 0);
    }

    pub fn orr_x(&mut self, rd: u8, rn: u8, rm: u8) {
        self.binop(rd, rn, rm, Width::X64, 6, 0);
    }

    pub fn eor_w(&mut self, rd: u8, rn: u8, rm: u8) {
        self.binop(rd, rn, rm, Width::W32, 4, 0);
    }

    pub fn eor_x(&mut self, rd: u8, rn: u8, rm: u8) {
        self.binop(rd, rn, rm, Width::X64, 4, 0);
    }

    pub fn lslv_w(&mut self, rd: u8, rn: u8, rm: u8) {
        self.binop(rd, rn, rm, Width::W32, 1, 0);
    }

    pub fn lslv_x(&mut self, rd: u8, rn: u8, rm: u8) {
        self.binop(rd, rn, rm, Width::X64, 1, 0);
    }

    pub fn lsrv_w(&mut self, rd: u8, rn: u8, rm: u8) {
        self.binop(rd, rn, rm, Width::W32, 5, 0);
    }

    pub fn lsrv_x(&mut self, rd: u8, rn: u8, rm: u8) {
        self.binop(rd, rn, rm, Width::X64, 5, 0);
    }

    pub fn asrv_w(&mut self, rd: u8, rn: u8, rm: u8) {
        self.binop(rd, rn, rm, Width::W32, 5, 0x20);
    }

    pub fn asrv_x(&mut self, rd: u8, rn: u8, rm: u8) {
        self.binop(rd, rn, rm, Width::X64, 5, 0x20);
    }

    pub fn rorv_w(&mut self, rd: u8, rn: u8, rm: u8) {
        self.rotate_right(rd, rn, rm, Width::W32);
    }

    pub fn rorv_x(&mut self, rd: u8, rn: u8, rm: u8) {
        self.rotate_right(rd, rn, rm, Width::X64);
    }

    pub fn rbit_w(&mut self, rd: u8, rn: u8) {
        self.pending_rbit = Some((false, rd, rn));
    }

    pub fn rbit_x(&mut self, rd: u8, rn: u8) {
        self.pending_rbit = Some((true, rd, rn));
    }

    pub fn clz_w(&mut self, rd: u8, rn: u8) {
        if self.pending_rbit.take() == Some((false, rd, rn)) {
            self.ctz(rd, rn, Width::W32);
        } else {
            self.clz(rd, rn, Width::W32);
        }
    }

    pub fn clz_x(&mut self, rd: u8, rn: u8) {
        if self.pending_rbit.take() == Some((true, rd, rn)) {
            self.ctz(rd, rn, Width::X64);
        } else {
            self.clz(rd, rn, Width::X64);
        }
    }

    pub fn sxth_w(&mut self, rd: u8, rn: u8) {
        self.load_slot32(T0, rn).expect("virtual register fits");
        self.slli(T0, T0, 48);
        self.srai(T0, T0, 48);
        self.store_slot32(rd, T0).expect("virtual register fits");
    }

    pub fn sxtb_w(&mut self, rd: u8, rn: u8) {
        self.load_slot32(T0, rn).expect("virtual register fits");
        self.slli(T0, T0, 56);
        self.srai(T0, T0, 56);
        self.store_slot32(rd, T0).expect("virtual register fits");
    }

    pub fn cmp_w(&mut self, rn: u8, rm: u8) {
        self.last_cmp = Some(LastCmp::Int {
            width: Width::W32,
            lhs: rn,
            rhs: Rhs::Reg(rm),
        });
    }

    pub fn cmp_x(&mut self, rn: u8, rm: u8) {
        self.last_cmp = Some(LastCmp::Int {
            width: Width::X64,
            lhs: rn,
            rhs: Rhs::Reg(rm),
        });
    }

    pub fn cmp_w_imm(&mut self, rn: u8, imm: u32) -> AsmResult {
        self.last_cmp = Some(LastCmp::Int {
            width: Width::W32,
            lhs: rn,
            rhs: Rhs::Imm32(imm),
        });
        Ok(())
    }

    pub fn cmp_w_u32(&mut self, rn: u8, imm: u32) {
        self.cmp_w_imm(rn, imm).expect("u32 immediate compares");
    }

    pub fn cset_w(&mut self, rd: u8, cond: Cond) {
        let cmp = self.last_cmp.take().expect("cset follows cmp");
        self.emit_cmp_bool(T0, cmp, cond)
            .expect("comparison operands fit");
        self.store_slot32(rd, T0).expect("virtual register fits");
    }

    pub fn csel_w(&mut self, rd: u8, rn: u8, rm: u8, cond: Cond) {
        let cmp = self.last_cmp.take().expect("csel follows cmp");
        self.emit_cmp_bool(T0, cmp, cond)
            .expect("comparison operands fit");
        self.load_slot32(T1, rn).expect("virtual register fits");
        self.load_slot32(T2, rm).expect("virtual register fits");
        self.insn(encode_b(0b001, T0, ZERO, 8).expect("local branch fits"));
        self.insn(encode_i(0x13, 0, T1, T2, 0).expect("mv encodes"));
        self.store_slot32(rd, T1).expect("virtual register fits");
    }

    pub fn fmov_s_from_w(&mut self, vd: u8, rn: u8) {
        self.mov_w(vd, rn);
    }

    pub fn fmov_w_from_s(&mut self, rd: u8, vn: u8) {
        self.mov_w(rd, vn);
    }

    pub fn fmov_d_from_x(&mut self, vd: u8, rn: u8) {
        self.mov_x(vd, rn);
    }

    pub fn fmov_x_from_d(&mut self, rd: u8, vn: u8) {
        self.mov_x(rd, vn);
    }

    pub fn fcmp_s(&mut self, vn: u8, vm: u8) {
        self.last_cmp = Some(LastCmp::Float {
            width: FloatWidth::F32,
            lhs: vn,
            rhs: vm,
        });
    }

    pub fn fcmp_d(&mut self, vn: u8, vm: u8) {
        self.last_cmp = Some(LastCmp::Float {
            width: FloatWidth::F64,
            lhs: vn,
            rhs: vm,
        });
    }

    pub fn fadd_d(&mut self, vd: u8, vn: u8, vm: u8) {
        self.float_binop(vd, vn, vm, true, 0x01, 0);
    }

    pub fn fadd_s(&mut self, vd: u8, vn: u8, vm: u8) {
        self.float_binop(vd, vn, vm, false, 0x00, 0);
    }

    pub fn fsub_d(&mut self, vd: u8, vn: u8, vm: u8) {
        self.float_binop(vd, vn, vm, true, 0x05, 0);
    }

    pub fn fsub_s(&mut self, vd: u8, vn: u8, vm: u8) {
        self.float_binop(vd, vn, vm, false, 0x04, 0);
    }

    pub fn fmul_d(&mut self, vd: u8, vn: u8, vm: u8) {
        self.float_binop(vd, vn, vm, true, 0x09, 0);
    }

    pub fn fmul_s(&mut self, vd: u8, vn: u8, vm: u8) {
        self.float_binop(vd, vn, vm, false, 0x08, 0);
    }

    pub fn fdiv_d(&mut self, vd: u8, vn: u8, vm: u8) {
        self.float_binop(vd, vn, vm, true, 0x0d, 0);
    }

    pub fn fdiv_s(&mut self, vd: u8, vn: u8, vm: u8) {
        self.float_binop(vd, vn, vm, false, 0x0c, 0);
    }

    pub fn fabs_s(&mut self, vd: u8, vn: u8) {
        self.load_slot32(T0, vn).expect("virtual register fits");
        self.li(T1, 0x7fff_ffff);
        self.insn(encode_r(0x33, 7, 0, T0, T0, T1));
        self.store_slot32(vd, T0).expect("virtual register fits");
    }

    pub fn fabs_d(&mut self, vd: u8, vn: u8) {
        self.load_slot64(T0, vn).expect("virtual register fits");
        self.li(T1, 0x7fff_ffff_ffff_ffff);
        self.insn(encode_r(0x33, 7, 0, T0, T0, T1));
        self.store_slot64(vd, T0).expect("virtual register fits");
    }

    pub fn fneg_s(&mut self, vd: u8, vn: u8) {
        self.load_slot32(T0, vn).expect("virtual register fits");
        self.li(T1, 0x8000_0000u32 as i32 as i64);
        self.insn(encode_r(0x33, 4, 0, T0, T0, T1));
        self.store_slot32(vd, T0).expect("virtual register fits");
    }

    pub fn fneg_d(&mut self, vd: u8, vn: u8) {
        self.load_slot64(T0, vn).expect("virtual register fits");
        self.li(T1, 0x8000_0000_0000_0000u64 as i64);
        self.insn(encode_r(0x33, 4, 0, T0, T0, T1));
        self.store_slot64(vd, T0).expect("virtual register fits");
    }

    pub fn fsqrt_s(&mut self, vd: u8, vn: u8) {
        self.float_unop(vd, vn, false, 0x2c, 0);
    }

    pub fn fsqrt_d(&mut self, vd: u8, vn: u8) {
        self.float_unop(vd, vn, true, 0x2d, 0);
    }

    pub fn frintn_s(&mut self, vd: u8, vn: u8) {
        self.round(vd, vn, false, 0);
    }

    pub fn frintn_d(&mut self, vd: u8, vn: u8) {
        self.round(vd, vn, true, 0);
    }

    pub fn frintp_s(&mut self, vd: u8, vn: u8) {
        self.round(vd, vn, false, 3);
    }

    pub fn frintp_d(&mut self, vd: u8, vn: u8) {
        self.round(vd, vn, true, 3);
    }

    pub fn frintm_s(&mut self, vd: u8, vn: u8) {
        self.round(vd, vn, false, 2);
    }

    pub fn frintm_d(&mut self, vd: u8, vn: u8) {
        self.round(vd, vn, true, 2);
    }

    pub fn frintz_s(&mut self, vd: u8, vn: u8) {
        self.round(vd, vn, false, 1);
    }

    pub fn frintz_d(&mut self, vd: u8, vn: u8) {
        self.round(vd, vn, true, 1);
    }

    pub fn cvtf_d_from_w(&mut self, vd: u8, rn: u8, signed: bool) {
        self.cvt_int_to_float(vd, rn, true, false, signed);
    }

    pub fn cvtf_s_from_w(&mut self, vd: u8, rn: u8, signed: bool) {
        self.cvt_int_to_float(vd, rn, false, false, signed);
    }

    pub fn cvtf_d_from_x(&mut self, vd: u8, rn: u8, signed: bool) {
        self.cvt_int_to_float(vd, rn, true, true, signed);
    }

    pub fn cvtf_s_from_x(&mut self, vd: u8, rn: u8, signed: bool) {
        self.cvt_int_to_float(vd, rn, false, true, signed);
    }

    pub fn fcvt_s_from_d(&mut self, vd: u8, vn: u8) {
        self.load_fslot(0, vn, true).expect("virtual register fits");
        self.insn(encode_fp(0x20, 0, 0, 0, 0, 1, 0));
        self.store_fslot(0, vd, false)
            .expect("virtual register fits");
    }

    pub fn fcvt_d_from_s(&mut self, vd: u8, vn: u8) {
        self.load_fslot(0, vn, false)
            .expect("virtual register fits");
        self.insn(encode_fp(0x21, 0, 0, 0, 0, 0, 0));
        self.store_fslot(0, vd, true)
            .expect("virtual register fits");
    }

    pub fn fcvt_w_from_s(&mut self, rd: u8, vn: u8, signed: bool) {
        self.fcvt_w(rd, vn, false, signed);
    }

    pub fn fcvt_w_from_d(&mut self, rd: u8, vn: u8, signed: bool) {
        self.fcvt_w(rd, vn, true, signed);
    }

    pub fn lsl_w_imm(&mut self, rd: u8, rn: u8, shift: u32) {
        self.load_slot32(T0, rn).expect("virtual register fits");
        self.insn(encode_i(0x1b, 1, T0, T0, (shift & 31) as i32).expect("slliw encodes"));
        self.store_slot32(rd, T0).expect("virtual register fits");
    }

    pub fn lsr_w_imm(&mut self, rd: u8, rn: u8, shift: u32) {
        self.load_slot32(T0, rn).expect("virtual register fits");
        self.insn(encode_i(0x1b, 5, T0, T0, (shift & 31) as i32).expect("srliw encodes"));
        self.store_slot32(rd, T0).expect("virtual register fits");
    }

    pub fn asr_w_imm(&mut self, rd: u8, rn: u8, shift: u32) {
        self.load_slot32(T0, rn).expect("virtual register fits");
        self.insn(
            encode_i(0x1b, 5, T0, T0, ((0x20 << 5) | (shift & 31)) as i32).expect("sraiw encodes"),
        );
        self.store_slot32(rd, T0).expect("virtual register fits");
    }

    pub fn ubfm_w(&mut self, rd: u8, rn: u8, immr: u32, imms: u32) {
        if immr == 0 {
            self.load_slot32(T0, rn).expect("virtual register fits");
            let mask = if imms >= 31 {
                u32::MAX
            } else {
                (1u32 << (imms + 1)) - 1
            };
            self.li(T1, mask as i32 as i64);
            self.insn(encode_r(0x33, 7, 0, T0, T0, T1));
            self.store_slot32(rd, T0).expect("virtual register fits");
        } else {
            self.lsr_w_imm(rd, rn, immr);
        }
    }

    pub fn ubfm_x(&mut self, rd: u8, rn: u8, immr: u32, imms: u32) {
        if immr == 0 {
            self.load_slot64(T0, rn).expect("virtual register fits");
            if imms < 63 {
                self.li(T1, ((1u64 << (imms + 1)) - 1) as i64);
                self.insn(encode_r(0x33, 7, 0, T0, T0, T1));
            }
            self.store_slot64(rd, T0).expect("virtual register fits");
        } else {
            self.lsr_x_imm(rd, rn, immr).expect("shift fits");
        }
    }

    pub fn sbfm_w(&mut self, rd: u8, rn: u8, immr: u32, imms: u32) {
        if immr == 0 && imms < 31 {
            let shift = 31 - imms;
            self.lsl_w_imm(rd, rn, shift);
            self.asr_w_imm(rd, rd, shift);
        } else {
            self.asr_w_imm(rd, rn, immr);
        }
    }

    pub fn sbfm_x(&mut self, rd: u8, rn: u8, immr: u32, imms: u32) {
        if immr == 0 && imms < 63 {
            let shift = 63 - imms;
            self.lsl_x_imm(rd, rn, shift).expect("shift fits");
            self.asr_x_imm(rd, rd, shift);
        } else {
            self.asr_x_imm(rd, rn, immr);
        }
    }

    pub fn lsl_x_imm(&mut self, rd: u8, rn: u8, shift: u32) -> AsmResult {
        if shift > 63 {
            return Err(AsmError::InvalidImmediate);
        }
        self.load_slot64(T0, rn)?;
        self.slli(T0, T0, shift);
        self.store_slot64(rd, T0)
    }

    pub fn lsr_x_imm(&mut self, rd: u8, rn: u8, shift: u32) -> AsmResult {
        if shift > 63 {
            return Err(AsmError::InvalidImmediate);
        }
        self.load_slot64(T0, rn)?;
        self.srli(T0, T0, shift);
        self.store_slot64(rd, T0)
    }

    pub fn ldr_x_imm(&mut self, rt: u8, rn: u8, offset: usize) -> AsmResult {
        self.load_slot64(T0, rn)?;
        self.load_mem64(
            T0,
            T0,
            i32::try_from(offset).map_err(|_| AsmError::InvalidImmediate)?,
        )?;
        self.store_slot64(rt, T0)
    }

    pub fn ldr_w_imm(&mut self, rt: u8, rn: u8, offset: usize) -> AsmResult {
        self.load_slot64(T0, rn)?;
        self.load_mem32(
            T0,
            T0,
            i32::try_from(offset).map_err(|_| AsmError::InvalidImmediate)?,
        )?;
        self.store_slot32(rt, T0)
    }

    pub fn ldr_w_unscaled_imm(&mut self, rt: u8, rn: u8, offset: u32) -> AsmResult {
        self.ldr_w_imm(rt, rn, offset as usize)
    }

    pub fn ldrb_w(&mut self, rt: u8, rn: u8) {
        self.load_mem_to_slot(rt, rn, 1, false);
    }

    pub fn ldrsb_w(&mut self, rt: u8, rn: u8) {
        self.load_mem_to_slot(rt, rn, 1, true);
    }

    pub fn ldrh_w(&mut self, rt: u8, rn: u8) {
        self.load_mem_to_slot(rt, rn, 2, false);
    }

    pub fn ldrsh_w(&mut self, rt: u8, rn: u8) {
        self.load_mem_to_slot(rt, rn, 2, true);
    }

    pub fn ldr_w(&mut self, rt: u8, rn: u8) {
        self.load_mem_to_slot(rt, rn, 4, false);
    }

    pub fn strb_w(&mut self, rt: u8, rn: u8) {
        self.store_slot_to_mem(rt, rn, 1);
    }

    pub fn strh_w(&mut self, rt: u8, rn: u8) {
        self.store_slot_to_mem(rt, rn, 2);
    }

    pub fn str_w(&mut self, rt: u8, rn: u8) {
        self.store_slot_to_mem(rt, rn, 4);
    }

    pub fn str_x_imm(&mut self, rt: u8, rn: u8, offset: usize) -> AsmResult {
        self.load_slot64(T0, rn)?;
        self.load_slot64(T1, rt)?;
        self.store_mem64(
            T0,
            i32::try_from(offset).map_err(|_| AsmError::InvalidImmediate)?,
            T1,
        )
    }

    pub fn str_w_imm(&mut self, rt: u8, rn: u8, offset: usize) -> AsmResult {
        self.load_slot64(T0, rn)?;
        self.load_slot32(T1, rt)?;
        self.store_mem32(
            T0,
            i32::try_from(offset).map_err(|_| AsmError::InvalidImmediate)?,
            T1,
        )
    }

    pub fn str_w_unscaled_imm(&mut self, rt: u8, rn: u8, offset: u32) -> AsmResult {
        self.str_w_imm(rt, rn, offset as usize)
    }

    fn insn(&mut self, insn: u32) {
        self.bytes.extend_from_slice(&insn.to_le_bytes());
        self.pending_rbit = None;
    }

    fn slot_disp(reg: u8) -> AsmResult<i32> {
        if usize::from(reg) < VREGS {
            Ok(-((i32::from(reg) + 1) * SLOT_SIZE))
        } else {
            Err(AsmError::InvalidRegister)
        }
    }

    fn addi(&mut self, rd: u8, rs1: u8, imm: i32) -> AsmResult {
        self.insn(encode_i(0x13, 0, rd, rs1, imm).ok_or(AsmError::InvalidImmediate)?);
        Ok(())
    }

    fn li(&mut self, rd: u8, value: i64) {
        if (-2048..=2047).contains(&value) {
            self.addi(rd, ZERO, value as i32).expect("small li fits");
            return;
        }
        let low = ((value << 52) >> 52) as i32;
        let high = (value - i64::from(low)) >> 12;
        self.li(rd, high);
        self.slli(rd, rd, 12);
        if low != 0 {
            self.addi(rd, rd, low).expect("low li fits");
        }
    }

    fn slli(&mut self, rd: u8, rs1: u8, shift: u32) {
        self.insn(encode_i(0x13, 1, rd, rs1, shift as i32).expect("slli encodes"));
    }

    fn srli(&mut self, rd: u8, rs1: u8, shift: u32) {
        self.insn(encode_i(0x13, 5, rd, rs1, shift as i32).expect("srli encodes"));
    }

    fn srai(&mut self, rd: u8, rs1: u8, shift: u32) {
        self.insn(encode_i(0x13, 5, rd, rs1, ((0x20 << 5) | shift) as i32).expect("srai encodes"));
    }

    fn asr_x_imm(&mut self, rd: u8, rn: u8, shift: u32) {
        self.load_slot64(T0, rn).expect("virtual register fits");
        self.srai(T0, T0, shift & 63);
        self.store_slot64(rd, T0).expect("virtual register fits");
    }

    fn load_slot64(&mut self, dst: u8, slot: u8) -> AsmResult {
        self.load_mem64(dst, S0, Self::slot_disp(slot)?)
    }

    fn load_slot32(&mut self, dst: u8, slot: u8) -> AsmResult {
        self.load_mem32(dst, S0, Self::slot_disp(slot)?)
    }

    fn store_slot64(&mut self, slot: u8, src: u8) -> AsmResult {
        self.store_mem64(S0, Self::slot_disp(slot)?, src)
    }

    fn store_slot32(&mut self, slot: u8, src: u8) -> AsmResult {
        let tmp = if src == T4 { T3 } else { T4 };
        self.slli(tmp, src, 32);
        self.srli(tmp, tmp, 32);
        self.store_mem64(S0, Self::slot_disp(slot)?, tmp)
    }

    fn load_mem64(&mut self, dst: u8, base: u8, offset: i32) -> AsmResult {
        self.load_mem(dst, base, offset, 3, true)
    }

    fn load_mem32(&mut self, dst: u8, base: u8, offset: i32) -> AsmResult {
        self.load_mem(dst, base, offset, 6, false)
    }

    fn load_mem(&mut self, dst: u8, base: u8, offset: i32, funct3: u32, signed: bool) -> AsmResult {
        let _ = signed;
        if let Some(insn) = encode_i(0x03, funct3, dst, base, offset) {
            self.insn(insn);
            return Ok(());
        }
        self.li(T4, i64::from(offset));
        self.insn(encode_r(0x33, 0, 0, T4, base, T4));
        self.insn(encode_i(0x03, funct3, dst, T4, 0).expect("zero load offset fits"));
        Ok(())
    }

    fn store_mem64(&mut self, base: u8, offset: i32, src: u8) -> AsmResult {
        self.store_mem(base, offset, src, 3)
    }

    fn store_mem32(&mut self, base: u8, offset: i32, src: u8) -> AsmResult {
        self.store_mem(base, offset, src, 2)
    }

    fn store_mem(&mut self, base: u8, offset: i32, src: u8, funct3: u32) -> AsmResult {
        if let Some(insn) = encode_s(0x23, funct3, base, src, offset) {
            self.insn(insn);
            return Ok(());
        }
        self.li(T4, i64::from(offset));
        self.insn(encode_r(0x33, 0, 0, T4, base, T4));
        self.insn(encode_s(0x23, funct3, T4, src, 0).expect("zero store offset fits"));
        Ok(())
    }

    fn binop(&mut self, rd: u8, rn: u8, rm: u8, width: Width, funct3: u32, funct7: u32) {
        let opcode = match width {
            Width::W32 => 0x3b,
            Width::X64 => 0x33,
        };
        match width {
            Width::W32 => {
                self.load_slot32(T0, rn).expect("virtual register fits");
                self.load_slot32(T1, rm).expect("virtual register fits");
                self.insn(encode_r(opcode, funct3, funct7, T0, T0, T1));
                self.store_slot32(rd, T0).expect("virtual register fits");
            }
            Width::X64 => {
                self.load_slot64(T0, rn).expect("virtual register fits");
                self.load_slot64(T1, rm).expect("virtual register fits");
                self.insn(encode_r(opcode, funct3, funct7, T0, T0, T1));
                self.store_slot64(rd, T0).expect("virtual register fits");
            }
        }
    }

    fn rotate_right(&mut self, rd: u8, rn: u8, rm: u8, width: Width) {
        match width {
            Width::W32 => {
                self.load_slot32(T0, rn).expect("virtual register fits");
                self.load_slot32(T1, rm).expect("virtual register fits");
                self.insn(encode_r(0x3b, 5, 0, T2, T0, T1));
                self.insn(encode_r(0x3b, 0, 0x20, T1, ZERO, T1));
                self.insn(encode_r(0x3b, 1, 0, T0, T0, T1));
                self.insn(encode_r(0x33, 6, 0, T0, T0, T2));
                self.store_slot32(rd, T0).expect("virtual register fits");
            }
            Width::X64 => {
                self.load_slot64(T0, rn).expect("virtual register fits");
                self.load_slot64(T1, rm).expect("virtual register fits");
                self.insn(encode_r(0x33, 5, 0, T2, T0, T1));
                self.insn(encode_r(0x33, 0, 0x20, T1, ZERO, T1));
                self.insn(encode_r(0x33, 1, 0, T0, T0, T1));
                self.insn(encode_r(0x33, 6, 0, T0, T0, T2));
                self.store_slot64(rd, T0).expect("virtual register fits");
            }
        }
    }

    fn clz(&mut self, rd: u8, rn: u8, width: Width) {
        let bits = match width {
            Width::W32 => 32,
            Width::X64 => 64,
        };
        self.load_width(T0, rn, width)
            .expect("virtual register fits");
        self.addi(T1, ZERO, bits).expect("bits fits");
        let done_zero = self.offset();
        self.insn(encode_b(0b000, T0, ZERO, 0).expect("placeholder fits"));
        self.addi(T1, ZERO, 0).expect("zero fits");
        self.li(T2, 1i64 << (bits - 1));
        let loop_start = self.offset();
        self.insn(encode_r(0x33, 7, 0, T3, T0, T2));
        self.insn(encode_b(0b001, T3, ZERO, 0).expect("placeholder fits"));
        let done_nonzero = self.offset() - 4;
        self.addi(T1, T1, 1).expect("inc fits");
        self.slli(T0, T0, 1);
        self.insn(
            encode_jal(ZERO, loop_start as isize - self.offset() as isize).expect("loop fits"),
        );
        let done = self.offset();
        self.patch_b(done_zero, done);
        self.patch_b(done_nonzero, done);
        self.store_slot32(rd, T1).expect("virtual register fits");
    }

    fn ctz(&mut self, rd: u8, rn: u8, width: Width) {
        let bits = match width {
            Width::W32 => 32,
            Width::X64 => 64,
        };
        self.load_width(T0, rn, width)
            .expect("virtual register fits");
        self.addi(T1, ZERO, bits).expect("bits fits");
        let done_zero = self.offset();
        self.insn(encode_b(0b000, T0, ZERO, 0).expect("placeholder fits"));
        self.addi(T1, ZERO, 0).expect("zero fits");
        let loop_start = self.offset();
        self.insn(encode_i(0x13, 7, T3, T0, 1).expect("andi fits"));
        self.insn(encode_b(0b001, T3, ZERO, 0).expect("placeholder fits"));
        let done_nonzero = self.offset() - 4;
        self.addi(T1, T1, 1).expect("inc fits");
        self.srli(T0, T0, 1);
        self.insn(
            encode_jal(ZERO, loop_start as isize - self.offset() as isize).expect("loop fits"),
        );
        let done = self.offset();
        self.patch_b(done_zero, done);
        self.patch_b(done_nonzero, done);
        self.store_slot32(rd, T1).expect("virtual register fits");
    }

    fn load_width(&mut self, dst: u8, slot: u8, width: Width) -> AsmResult {
        match width {
            Width::W32 => self.load_slot32(dst, slot),
            Width::X64 => self.load_slot64(dst, slot),
        }
    }

    fn patch_b(&mut self, at: usize, target: usize) {
        let delta = target as isize - at as isize;
        let old = u32::from_le_bytes(self.bytes[at..at + 4].try_into().expect("branch slot"));
        let funct3 = (old >> 12) & 0x7;
        let rs1 = ((old >> 15) & 0x1f) as u8;
        let rs2 = ((old >> 20) & 0x1f) as u8;
        let insn = encode_b(funct3, rs1, rs2, delta).expect("local branch fits");
        self.bytes[at..at + 4].copy_from_slice(&insn.to_le_bytes());
    }

    fn load_cmp_operands(&mut self, cmp: LastCmp) -> AsmResult<(Width, Option<FloatWidth>)> {
        match cmp {
            LastCmp::Int { width, lhs, rhs } => {
                self.load_width(T0, lhs, width)?;
                match rhs {
                    Rhs::Reg(reg) => self.load_width(T1, reg, width)?,
                    Rhs::Imm32(imm) => self.li(T1, i64::from(imm as i32)),
                }
                Ok((width, None))
            }
            LastCmp::Float { width, lhs, rhs } => {
                self.load_fslot(0, lhs, width == FloatWidth::F64)?;
                self.load_fslot(1, rhs, width == FloatWidth::F64)?;
                Ok((Width::X64, Some(width)))
            }
        }
    }

    fn emit_cmp_bool(&mut self, dst: u8, cmp: LastCmp, cond: Cond) -> AsmResult {
        let (width, float) = self.load_cmp_operands(cmp)?;
        if let Some(float_width) = float {
            return self.emit_float_cmp_bool(dst, float_width, cond);
        }
        if width == Width::W32 {
            self.slli(T0, T0, 32);
            self.srli(T0, T0, 32);
            self.slli(T1, T1, 32);
            self.srli(T1, T1, 32);
        }
        match cond {
            Cond::Eq => {
                self.insn(encode_r(0x33, 4, 0, dst, T0, T1));
                self.insn(encode_i(0x13, 3, dst, dst, 1).expect("sltiu fits"));
            }
            Cond::Ne => {
                self.insn(encode_r(0x33, 4, 0, dst, T0, T1));
                self.insn(encode_r(0x33, 3, 0, dst, ZERO, dst));
            }
            Cond::Lo => self.insn(encode_r(0x33, 3, 0, dst, T0, T1)),
            Cond::Hs => {
                self.insn(encode_r(0x33, 3, 0, dst, T0, T1));
                self.insn(encode_i(0x13, 4, dst, dst, 1).expect("xori fits"));
            }
            Cond::Hi => self.insn(encode_r(0x33, 3, 0, dst, T1, T0)),
            Cond::Ls => {
                self.insn(encode_r(0x33, 3, 0, dst, T1, T0));
                self.insn(encode_i(0x13, 4, dst, dst, 1).expect("xori fits"));
            }
            Cond::Lt => self.insn(encode_r(0x33, 2, 0, dst, T0, T1)),
            Cond::Ge => {
                self.insn(encode_r(0x33, 2, 0, dst, T0, T1));
                self.insn(encode_i(0x13, 4, dst, dst, 1).expect("xori fits"));
            }
            Cond::Gt => self.insn(encode_r(0x33, 2, 0, dst, T1, T0)),
            Cond::Le => {
                self.insn(encode_r(0x33, 2, 0, dst, T1, T0));
                self.insn(encode_i(0x13, 4, dst, dst, 1).expect("xori fits"));
            }
        }
        Ok(())
    }

    fn emit_cmp_branch_to_skip(&mut self, cmp: LastCmp, inverted_cond: Cond) -> AsmResult {
        self.emit_cmp_bool(T4, cmp, inverted_cond)?;
        self.insn(encode_b(0b001, T4, ZERO, 8).expect("local branch fits"));
        Ok(())
    }

    fn emit_float_cmp_bool(&mut self, dst: u8, width: FloatWidth, cond: Cond) -> AsmResult {
        let fmt = match width {
            FloatWidth::F32 => 0x50,
            FloatWidth::F64 => 0x51,
        };
        match cond {
            Cond::Eq => self.insn(encode_fp(fmt, 2, dst, 0, 1, 0, 0)),
            Cond::Ne => {
                self.insn(encode_fp(fmt, 2, dst, 0, 1, 0, 0));
                self.insn(encode_i(0x13, 4, dst, dst, 1).expect("xori fits"));
            }
            Cond::Lo | Cond::Lt => self.insn(encode_fp(fmt, 1, dst, 0, 1, 0, 0)),
            Cond::Gt => self.insn(encode_fp(fmt, 1, dst, 1, 0, 0, 0)),
            Cond::Ls | Cond::Le => self.insn(encode_fp(fmt, 0, dst, 0, 1, 0, 0)),
            Cond::Ge => self.insn(encode_fp(fmt, 0, dst, 1, 0, 0, 0)),
            _ => return Err(AsmError::InvalidImmediate),
        }
        Ok(())
    }

    fn load_fslot(&mut self, fd: u8, slot: u8, f64: bool) -> AsmResult {
        let offset = Self::slot_disp(slot)?;
        let funct3 = if f64 { 3 } else { 2 };
        self.insn(encode_i(0x07, funct3, fd, S0, offset).ok_or(AsmError::InvalidImmediate)?);
        Ok(())
    }

    fn store_fslot(&mut self, fs: u8, slot: u8, f64: bool) -> AsmResult {
        let offset = Self::slot_disp(slot)?;
        let funct3 = if f64 { 3 } else { 2 };
        self.insn(encode_s(0x27, funct3, S0, fs, offset).ok_or(AsmError::InvalidImmediate)?);
        Ok(())
    }

    fn float_binop(&mut self, vd: u8, vn: u8, vm: u8, f64: bool, funct7: u32, rm: u32) {
        self.load_fslot(0, vn, f64).expect("virtual register fits");
        self.load_fslot(1, vm, f64).expect("virtual register fits");
        self.insn(encode_fp(funct7, rm, 0, 0, 1, 0, 0));
        self.store_fslot(0, vd, f64).expect("virtual register fits");
    }

    fn float_unop(&mut self, vd: u8, vn: u8, f64: bool, funct7: u32, rm: u32) {
        self.load_fslot(0, vn, f64).expect("virtual register fits");
        self.insn(encode_fp(funct7, rm, 0, 0, 0, 0, 0));
        self.store_fslot(0, vd, f64).expect("virtual register fits");
    }

    fn round(&mut self, vd: u8, vn: u8, f64: bool, rm: u32) {
        self.load_fslot(0, vn, f64).expect("virtual register fits");
        self.insn(encode_fp(if f64 { 0x61 } else { 0x60 }, rm, T0, 0, 2, 0, 0));
        self.insn(encode_fp(if f64 { 0x69 } else { 0x68 }, 0, 0, T0, 2, 0, 0));
        self.store_fslot(0, vd, f64).expect("virtual register fits");
    }

    fn cvt_int_to_float(&mut self, vd: u8, rn: u8, to_f64: bool, from_i64: bool, signed: bool) {
        if from_i64 {
            self.load_slot64(T0, rn).expect("virtual register fits");
        } else if signed {
            self.load_slot32(T0, rn).expect("virtual register fits");
            self.slli(T0, T0, 32);
            self.srai(T0, T0, 32);
        } else {
            self.load_slot32(T0, rn).expect("virtual register fits");
        }
        let rs2 = match (from_i64, signed) {
            (false, true) => 0,
            (false, false) => 1,
            (true, true) => 2,
            (true, false) => 3,
        };
        self.insn(encode_fp(
            if to_f64 { 0x69 } else { 0x68 },
            0,
            0,
            T0,
            rs2,
            0,
            0,
        ));
        self.store_fslot(0, vd, to_f64)
            .expect("virtual register fits");
    }

    fn fcvt_w(&mut self, rd: u8, vn: u8, f64: bool, signed: bool) {
        self.load_fslot(0, vn, f64).expect("virtual register fits");
        let rs2 = if signed { 0 } else { 1 };
        self.insn(encode_fp(
            if f64 { 0x61 } else { 0x60 },
            1,
            T0,
            0,
            rs2,
            0,
            0,
        ));
        self.store_slot32(rd, T0).expect("virtual register fits");
    }

    fn load_mem_to_slot(&mut self, rt: u8, rn: u8, width: u32, signed: bool) {
        self.load_slot64(T0, rn).expect("virtual register fits");
        let funct3 = match (width, signed) {
            (1, true) => 0,
            (1, false) => 4,
            (2, true) => 1,
            (2, false) => 5,
            (4, _) => 6,
            _ => unreachable!("invalid load width"),
        };
        self.load_mem(T1, T0, 0, funct3, signed)
            .expect("zero offset load fits");
        self.store_slot32(rt, T1).expect("virtual register fits");
    }

    fn store_slot_to_mem(&mut self, rt: u8, rn: u8, width: u32) {
        self.load_slot64(T0, rn).expect("virtual register fits");
        self.load_slot32(T1, rt).expect("virtual register fits");
        let funct3 = match width {
            1 => 0,
            2 => 1,
            4 => 2,
            _ => unreachable!("invalid store width"),
        };
        self.store_mem(T0, 0, T1, funct3)
            .expect("zero offset store fits");
    }
}

fn encode_i(opcode: u32, funct3: u32, rd: u8, rs1: u8, imm: i32) -> Option<u32> {
    if !(-2048..=2047).contains(&imm) {
        return None;
    }
    Some(
        (((imm as u32) & 0xfff) << 20)
            | ((rs1 as u32) << 15)
            | (funct3 << 12)
            | ((rd as u32) << 7)
            | opcode,
    )
}

fn encode_s(opcode: u32, funct3: u32, rs1: u8, rs2: u8, imm: i32) -> Option<u32> {
    if !(-2048..=2047).contains(&imm) {
        return None;
    }
    let imm = (imm as u32) & 0xfff;
    Some(
        ((imm >> 5) << 25)
            | ((rs2 as u32) << 20)
            | ((rs1 as u32) << 15)
            | (funct3 << 12)
            | ((imm & 0x1f) << 7)
            | opcode,
    )
}

fn encode_r(opcode: u32, funct3: u32, funct7: u32, rd: u8, rs1: u8, rs2: u8) -> u32 {
    (funct7 << 25)
        | ((rs2 as u32) << 20)
        | ((rs1 as u32) << 15)
        | (funct3 << 12)
        | ((rd as u32) << 7)
        | opcode
}

fn encode_b(funct3: u32, rs1: u8, rs2: u8, offset: isize) -> Option<u32> {
    if offset % 2 != 0 || !(-4096..=4094).contains(&offset) {
        return None;
    }
    let imm = (offset as i32 as u32) & 0x1fff;
    Some(
        ((imm >> 12) << 31)
            | (((imm >> 5) & 0x3f) << 25)
            | ((rs2 as u32) << 20)
            | ((rs1 as u32) << 15)
            | (funct3 << 12)
            | (((imm >> 1) & 0xf) << 8)
            | (((imm >> 11) & 0x1) << 7)
            | 0x63,
    )
}

fn encode_jal(rd: u8, offset: isize) -> Option<u32> {
    if offset % 2 != 0 || !(-(1 << 20)..=(1 << 20) - 2).contains(&offset) {
        return None;
    }
    let imm = (offset as i32 as u32) & 0x1f_ffff;
    Some(
        ((imm >> 20) << 31)
            | (((imm >> 1) & 0x3ff) << 21)
            | (((imm >> 11) & 0x1) << 20)
            | (((imm >> 12) & 0xff) << 12)
            | ((rd as u32) << 7)
            | 0x6f,
    )
}

fn encode_fp(funct7: u32, rm: u32, rd: u8, rs1: u8, rs2: u8, _fmt: u32, _unused: u32) -> u32 {
    (funct7 << 25)
        | ((rs2 as u32) << 20)
        | ((rs1 as u32) << 15)
        | (rm << 12)
        | ((rd as u32) << 7)
        | 0x53
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patches_jal_forward() {
        let mut bytes = encode_jal(ZERO, 0).unwrap().to_le_bytes().to_vec();
        patch_branch(&mut bytes, 0, 16, BranchKind::B).unwrap();
        assert_eq!(
            u32::from_le_bytes(bytes.try_into().unwrap()),
            encode_jal(ZERO, 16).unwrap()
        );
    }

    #[test]
    fn emits_ret_encoding() {
        let mut asm = Riscv64BaselineMasm::with_capacity(4);
        asm.ret();
        assert_eq!(
            asm.into_bytes(),
            encode_i(0x67, 0, ZERO, RA, 0).unwrap().to_le_bytes()
        );
    }

    #[test]
    fn emits_addw_virtual_slots() {
        let mut asm = Riscv64BaselineMasm::with_capacity(32);
        asm.add_w(2, 0, 1);
        let bytes = asm.into_bytes();
        assert!(bytes
            .windows(4)
            .any(|window| window == encode_r(0x3b, 0, 0, T0, T0, T1).to_le_bytes()));
    }

    #[test]
    fn emits_indirect_helper_call() {
        let mut asm = Riscv64BaselineMasm::with_capacity(128);
        asm.blr_x(16);
        assert!(asm
            .into_bytes()
            .windows(4)
            .any(|window| { window == encode_i(0x67, 0, RA, T0, 0).unwrap().to_le_bytes() }));
    }

    #[test]
    fn emits_load_store_instructions() {
        let mut asm = Riscv64BaselineMasm::with_capacity(128);
        asm.ldr_w_imm(1, 0, 12).unwrap();
        asm.str_w_imm(1, 0, 16).unwrap();
        let bytes = asm.into_bytes();
        assert!(bytes
            .windows(4)
            .any(|window| (u32::from_le_bytes(window.try_into().unwrap()) & 0x7f) == 0x03));
        assert!(bytes
            .windows(4)
            .any(|window| (u32::from_le_bytes(window.try_into().unwrap()) & 0x7f) == 0x23));
    }

    #[test]
    fn emits_float_add_instruction() {
        let mut asm = Riscv64BaselineMasm::with_capacity(64);
        asm.fadd_s(2, 0, 1);
        assert!(asm
            .into_bytes()
            .windows(4)
            .any(|window| u32::from_le_bytes(window.try_into().unwrap())
                == encode_fp(0x00, 0, 0, 0, 1, 0, 0)));
    }
}
