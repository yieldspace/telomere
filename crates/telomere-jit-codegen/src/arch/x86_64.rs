use crate::{
    masm::{AsmError, AsmResult},
    target::{TargetArch, TargetInfo, TargetOs},
};

const VREGS: usize = 32;
const SLOT_SIZE: i32 = 8;
const FRAME_SIZE: i32 = ((VREGS as i32 + 2) * SLOT_SIZE + 15) & !15;
const EXTRA_ARG6: i32 = -FRAME_SIZE;
const EXTRA_ARG7: i32 = -FRAME_SIZE + SLOT_SIZE;

const RAX: u8 = 0;
const RCX: u8 = 1;
const RDX: u8 = 2;
const RBP: u8 = 5;
const RSI: u8 = 6;
const RDI: u8 = 7;
const R8: u8 = 8;
const R9: u8 = 9;
const R10: u8 = 10;
const R11: u8 = 11;

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cond {
    Eq = 0x4,
    Ne = 0x5,
    Hs = 0x3,
    Lo = 0x2,
    Hi = 0x7,
    Ls = 0x6,
    Ge = 0xd,
    Lt = 0xc,
    Gt = 0xf,
    Le = 0xe,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BranchKind {
    B,
    BCond(Cond),
    CbnzX(u8),
    CbnzW(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FloatWidth {
    F32,
    F64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LastCmp {
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
        arch: TargetArch::X86_64,
        os: target_os(),
        baseline_supported: cfg!(all(
            any(target_os = "macos", target_os = "linux"),
            target_arch = "x86_64"
        )),
    }
}

pub fn sse41_rounding_supported() -> bool {
    detect_sse41_rounding()
}

#[cfg(target_arch = "x86_64")]
fn detect_sse41_rounding() -> bool {
    std::arch::is_x86_feature_detected!("sse4.1")
}

#[cfg(not(target_arch = "x86_64"))]
fn detect_sse41_rounding() -> bool {
    false
}

const fn target_os() -> TargetOs {
    #[cfg(target_os = "macos")]
    {
        TargetOs::Macos
    }
    #[cfg(target_os = "linux")]
    {
        TargetOs::Linux
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        TargetOs::Unsupported
    }
}

pub fn patch_branch(bytes: &mut [u8], at: usize, target: usize, kind: BranchKind) -> AsmResult {
    let (disp_at, insn_len) = match kind {
        BranchKind::B => (at.checked_add(1).ok_or(AsmError::InvalidImmediate)?, 5usize),
        BranchKind::BCond(_) | BranchKind::CbnzX(_) | BranchKind::CbnzW(_) => {
            (at.checked_add(2).ok_or(AsmError::InvalidImmediate)?, 6usize)
        }
    };
    let after = at.checked_add(insn_len).ok_or(AsmError::InvalidImmediate)?;
    let disp = (target as isize)
        .checked_sub(after as isize)
        .ok_or(AsmError::BranchOutOfRange)?;
    let disp = i32::try_from(disp).map_err(|_| AsmError::BranchOutOfRange)?;
    bytes
        .get_mut(disp_at..disp_at + 4)
        .ok_or(AsmError::InvalidImmediate)?
        .copy_from_slice(&disp.to_le_bytes());
    Ok(())
}

#[derive(Debug, Clone)]
pub struct X64BaselineMasm {
    bytes: Vec<u8>,
    prologue_emitted: bool,
    epilogue_emitted: bool,
    pending_rbit: Option<(bool, u8, u8)>,
    last_cmp: Option<LastCmp>,
}

impl X64BaselineMasm {
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
                self.emit(&[0xe9, 0, 0, 0, 0]);
                Ok(at)
            }
            BranchKind::BCond(cond) => {
                let at = self.offset();
                self.emit(&[0x0f, 0x80 | cond as u8, 0, 0, 0, 0]);
                Ok(at)
            }
            BranchKind::CbnzX(reg) => {
                self.load_slot64(RAX, reg)?;
                self.test_rr64(RAX, RAX);
                let at = self.offset();
                self.emit(&[0x0f, 0x85, 0, 0, 0, 0]);
                Ok(at)
            }
            BranchKind::CbnzW(reg) => {
                self.load_slot32(RAX, reg)?;
                self.test_rr32(RAX, RAX);
                let at = self.offset();
                self.emit(&[0x0f, 0x85, 0, 0, 0, 0]);
                Ok(at)
            }
        }
    }

    pub fn ret(&mut self) {
        self.bytes.push(0xc3);
    }

    pub fn mov_x_from_sp(&mut self, rd: u8) {
        let _ = rd;
    }

    pub fn blr_x(&mut self, rn: u8) {
        self.load_slot64(R11, rn).expect("virtual register fits");
        self.load_slot64(RDI, 0).expect("argument slot fits");
        self.load_slot64(RSI, 1).expect("argument slot fits");
        self.load_slot64(RDX, 2).expect("argument slot fits");
        self.load_slot64(RCX, 3).expect("argument slot fits");
        self.load_slot64(R8, 4).expect("argument slot fits");
        self.load_slot64(R9, 5).expect("argument slot fits");
        self.load_slot64(RAX, 6).expect("argument slot fits");
        self.store_frame_disp64(EXTRA_ARG6, RAX);
        self.load_slot64(RAX, 7).expect("argument slot fits");
        self.store_frame_disp64(EXTRA_ARG7, RAX);
        self.emit(&[0x41, 0xff, 0xd3]);
        self.store_slot64(0, RAX).expect("virtual register fits");
        self.store_slot64(1, RDX).expect("virtual register fits");
    }

    pub fn mov_x(&mut self, rd: u8, rn: u8) {
        self.load_slot64(RAX, rn).expect("virtual register fits");
        self.store_slot64(rd, RAX).expect("virtual register fits");
    }

    pub fn mov_w(&mut self, rd: u8, rn: u8) {
        self.load_slot32(RAX, rn).expect("virtual register fits");
        self.store_slot32(rd, RAX).expect("virtual register fits");
    }

    pub fn mov_imm_u32(&mut self, rd: u8, value: u32) {
        self.mov_imm32(RAX, value);
        self.store_slot32(rd, RAX).expect("virtual register fits");
    }

    pub fn mov_imm_u64(&mut self, rd: u8, value: u64) {
        self.mov_imm64(RAX, value);
        self.store_slot64(rd, RAX).expect("virtual register fits");
    }

    pub fn stp_pre_x_sp(&mut self, rt: u8, rt2: u8) {
        self.stp_pre(rt, rt2);
    }

    pub fn stp_pre(&mut self, rt: u8, rt2: u8) {
        if !self.prologue_emitted && rt == 29 && rt2 == 30 {
            self.prologue_emitted = true;
            self.emit(&[0x55]);
            self.emit(&[0x48, 0x89, 0xe5]);
            self.emit(&[0x48, 0x81, 0xec]);
            self.bytes
                .extend_from_slice(&(FRAME_SIZE as u32).to_le_bytes());
            for (slot, reg) in [RDI, RSI, RDX, RCX, R8, R9, R10, R11]
                .into_iter()
                .enumerate()
            {
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
            self.load_slot64(RAX, 0).expect("return slot fits");
            self.load_slot64(RDX, 1).expect("return slot fits");
            self.emit(&[0x48, 0x89, 0xec]);
            self.emit(&[0x5d]);
        }
    }

    pub fn add_imm_u32(&mut self, rd: u8, rn: u8, imm: u32) -> AsmResult {
        self.load_slot32(RAX, rn)?;
        self.alu_imm32(0x81, 0, RAX, imm);
        self.store_slot32(rd, RAX)
    }

    pub fn add_imm_u64(&mut self, rd: u8, rn: u8, imm: u64) -> AsmResult {
        self.load_slot64(RAX, rn)?;
        if let Ok(imm) = i32::try_from(imm) {
            self.alu_imm32_wide(0x81, 0, RAX, imm as u32);
        } else {
            self.mov_imm64(RCX, imm);
            self.alu_rm64(0x03, RAX, RCX);
        }
        self.store_slot64(rd, RAX)
    }

    pub fn sub_imm_u32(&mut self, rd: u8, rn: u8, imm: u32) -> AsmResult {
        self.load_slot32(RAX, rn)?;
        self.alu_imm32(0x81, 5, RAX, imm);
        self.store_slot32(rd, RAX)
    }

    pub fn add_w(&mut self, rd: u8, rn: u8, rm: u8) {
        self.binop_w(rd, rn, rm, 0x03)
            .expect("virtual register fits");
    }

    pub fn add_x(&mut self, rd: u8, rn: u8, rm: u8) {
        self.binop_x(rd, rn, rm, 0x03)
            .expect("virtual register fits");
    }

    pub fn sub_w(&mut self, rd: u8, rn: u8, rm: u8) {
        self.binop_w(rd, rn, rm, 0x2b)
            .expect("virtual register fits");
    }

    pub fn sub_x(&mut self, rd: u8, rn: u8, rm: u8) {
        self.binop_x(rd, rn, rm, 0x2b)
            .expect("virtual register fits");
    }

    pub fn mul_w(&mut self, rd: u8, rn: u8, rm: u8) {
        self.load_slot32(RAX, rn).expect("virtual register fits");
        self.imul_slot32(RAX, rm).expect("virtual register fits");
        self.store_slot32(rd, RAX).expect("virtual register fits");
    }

    pub fn mul_x(&mut self, rd: u8, rn: u8, rm: u8) {
        self.load_slot64(RAX, rn).expect("virtual register fits");
        self.imul_slot64(RAX, rm).expect("virtual register fits");
        self.store_slot64(rd, RAX).expect("virtual register fits");
    }

    pub fn udiv_w(&mut self, rd: u8, rn: u8, rm: u8) {
        self.load_slot32(RAX, rn).expect("virtual register fits");
        self.emit(&[0x31, 0xd2]);
        self.load_slot32(RCX, rm).expect("virtual register fits");
        self.emit_modrm_reg(0xf7, 6, RCX, false);
        self.store_slot32(rd, RAX).expect("virtual register fits");
    }

    pub fn sdiv_w(&mut self, rd: u8, rn: u8, rm: u8) {
        self.load_slot32(RAX, rn).expect("virtual register fits");
        self.emit(&[0x99]);
        self.load_slot32(RCX, rm).expect("virtual register fits");
        self.emit_modrm_reg(0xf7, 7, RCX, false);
        self.store_slot32(rd, RAX).expect("virtual register fits");
    }

    pub fn udiv_x(&mut self, rd: u8, rn: u8, rm: u8) {
        self.load_slot64(RAX, rn).expect("virtual register fits");
        self.emit(&[0x48, 0x31, 0xd2]);
        self.load_slot64(RCX, rm).expect("virtual register fits");
        self.emit_modrm_reg(0xf7, 6, RCX, true);
        self.store_slot64(rd, RAX).expect("virtual register fits");
    }

    pub fn sdiv_x(&mut self, rd: u8, rn: u8, rm: u8) {
        self.load_slot64(RAX, rn).expect("virtual register fits");
        self.emit(&[0x48, 0x99]);
        self.load_slot64(RCX, rm).expect("virtual register fits");
        self.emit_modrm_reg(0xf7, 7, RCX, true);
        self.store_slot64(rd, RAX).expect("virtual register fits");
    }

    pub fn msub_w(&mut self, rd: u8, rn: u8, rm: u8, ra: u8) {
        self.load_slot32(RAX, rn).expect("virtual register fits");
        self.imul_slot32(RAX, rm).expect("virtual register fits");
        self.load_slot32(RCX, ra).expect("virtual register fits");
        self.alu_rm32(0x2b, RCX, RAX);
        self.store_slot32(rd, RCX).expect("virtual register fits");
    }

    pub fn msub_x(&mut self, rd: u8, rn: u8, rm: u8, ra: u8) {
        self.load_slot64(RAX, rn).expect("virtual register fits");
        self.imul_slot64(RAX, rm).expect("virtual register fits");
        self.load_slot64(RCX, ra).expect("virtual register fits");
        self.alu_rm64(0x2b, RCX, RAX);
        self.store_slot64(rd, RCX).expect("virtual register fits");
    }

    pub fn neg_w(&mut self, rd: u8, rm: u8) {
        self.load_slot32(RAX, rm).expect("virtual register fits");
        self.emit_modrm_reg(0xf7, 3, RAX, false);
        self.store_slot32(rd, RAX).expect("virtual register fits");
    }

    pub fn neg_x(&mut self, rd: u8, rm: u8) {
        self.load_slot64(RAX, rm).expect("virtual register fits");
        self.emit_modrm_reg(0xf7, 3, RAX, true);
        self.store_slot64(rd, RAX).expect("virtual register fits");
    }

    pub fn and_w(&mut self, rd: u8, rn: u8, rm: u8) {
        self.binop_w(rd, rn, rm, 0x23)
            .expect("virtual register fits");
    }

    pub fn and_x(&mut self, rd: u8, rn: u8, rm: u8) {
        self.binop_x(rd, rn, rm, 0x23)
            .expect("virtual register fits");
    }

    pub fn orr_w(&mut self, rd: u8, rn: u8, rm: u8) {
        self.binop_w(rd, rn, rm, 0x0b)
            .expect("virtual register fits");
    }

    pub fn orr_x(&mut self, rd: u8, rn: u8, rm: u8) {
        self.binop_x(rd, rn, rm, 0x0b)
            .expect("virtual register fits");
    }

    pub fn eor_w(&mut self, rd: u8, rn: u8, rm: u8) {
        self.binop_w(rd, rn, rm, 0x33)
            .expect("virtual register fits");
    }

    pub fn eor_x(&mut self, rd: u8, rn: u8, rm: u8) {
        self.binop_x(rd, rn, rm, 0x33)
            .expect("virtual register fits");
    }

    pub fn lslv_w(&mut self, rd: u8, rn: u8, rm: u8) {
        self.shift_var(rd, rn, rm, false, 4);
    }

    pub fn lslv_x(&mut self, rd: u8, rn: u8, rm: u8) {
        self.shift_var(rd, rn, rm, true, 4);
    }

    pub fn lsrv_w(&mut self, rd: u8, rn: u8, rm: u8) {
        self.shift_var(rd, rn, rm, false, 5);
    }

    pub fn lsrv_x(&mut self, rd: u8, rn: u8, rm: u8) {
        self.shift_var(rd, rn, rm, true, 5);
    }

    pub fn asrv_w(&mut self, rd: u8, rn: u8, rm: u8) {
        self.shift_var(rd, rn, rm, false, 7);
    }

    pub fn asrv_x(&mut self, rd: u8, rn: u8, rm: u8) {
        self.shift_var(rd, rn, rm, true, 7);
    }

    pub fn rorv_w(&mut self, rd: u8, rn: u8, rm: u8) {
        self.shift_var(rd, rn, rm, false, 1);
    }

    pub fn rorv_x(&mut self, rd: u8, rn: u8, rm: u8) {
        self.shift_var(rd, rn, rm, true, 1);
    }

    pub fn rbit_w(&mut self, rd: u8, rn: u8) {
        self.pending_rbit = Some((false, rd, rn));
    }

    pub fn rbit_x(&mut self, rd: u8, rn: u8) {
        self.pending_rbit = Some((true, rd, rn));
    }

    pub fn clz_w(&mut self, rd: u8, rn: u8) {
        if self.pending_rbit.take() == Some((false, rd, rn)) {
            self.ctz(rd, rn, false);
        } else {
            self.clz(rd, rn, false);
        }
    }

    pub fn clz_x(&mut self, rd: u8, rn: u8) {
        if self.pending_rbit.take() == Some((true, rd, rn)) {
            self.ctz(rd, rn, true);
        } else {
            self.clz(rd, rn, true);
        }
    }

    pub fn sxth_w(&mut self, rd: u8, rn: u8) {
        self.movsx_slot32(rd, rn, 0xbf);
    }

    pub fn sxtb_w(&mut self, rd: u8, rn: u8) {
        self.movsx_slot32(rd, rn, 0xbe);
    }

    pub fn cmp_w(&mut self, rn: u8, rm: u8) {
        self.load_slot32(RAX, rn).expect("virtual register fits");
        self.cmp_slot32(RAX, rm).expect("virtual register fits");
        self.last_cmp = None;
    }

    pub fn cmp_x(&mut self, rn: u8, rm: u8) {
        self.load_slot64(RAX, rn).expect("virtual register fits");
        self.cmp_slot64(RAX, rm).expect("virtual register fits");
        self.last_cmp = None;
    }

    pub fn cmp_w_imm(&mut self, rn: u8, imm: u32) -> AsmResult {
        self.load_slot32(RAX, rn)?;
        self.alu_imm32(0x81, 7, RAX, imm);
        self.last_cmp = None;
        Ok(())
    }

    pub fn cmp_w_u32(&mut self, rn: u8, imm: u32) {
        self.cmp_w_imm(rn, imm).expect("u32 immediate is encodable");
    }

    pub fn cset_w(&mut self, rd: u8, cond: Cond) {
        if let Some(LastCmp::Float { width, lhs, rhs }) = self.last_cmp.take() {
            self.float_cset(rd, cond, width, lhs, rhs)
                .expect("virtual register fits");
            return;
        }
        self.emit(&[0x0f, 0x90 | cond as u8, 0xc0]);
        self.emit(&[0x0f, 0xb6, 0xc0]);
        self.store_slot32(rd, RAX).expect("virtual register fits");
    }

    pub fn csel_w(&mut self, rd: u8, rn: u8, rm: u8, cond: Cond) {
        self.load_slot32(RAX, rm).expect("virtual register fits");
        self.load_slot32(RCX, rn).expect("virtual register fits");
        self.emit_rex(false, RCX, RAX);
        self.emit(&[0x0f, 0x40 | cond as u8]);
        self.modrm_rr(RCX, RAX);
        self.store_slot32(rd, RAX).expect("virtual register fits");
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
        self.float_binop(vd, vn, vm, true, 0x58);
    }

    pub fn fadd_s(&mut self, vd: u8, vn: u8, vm: u8) {
        self.float_binop(vd, vn, vm, false, 0x58);
    }

    pub fn fsub_d(&mut self, vd: u8, vn: u8, vm: u8) {
        self.float_binop(vd, vn, vm, true, 0x5c);
    }

    pub fn fsub_s(&mut self, vd: u8, vn: u8, vm: u8) {
        self.float_binop(vd, vn, vm, false, 0x5c);
    }

    pub fn fmul_d(&mut self, vd: u8, vn: u8, vm: u8) {
        self.float_binop(vd, vn, vm, true, 0x59);
    }

    pub fn fmul_s(&mut self, vd: u8, vn: u8, vm: u8) {
        self.float_binop(vd, vn, vm, false, 0x59);
    }

    pub fn fdiv_d(&mut self, vd: u8, vn: u8, vm: u8) {
        self.float_binop(vd, vn, vm, true, 0x5e);
    }

    pub fn fdiv_s(&mut self, vd: u8, vn: u8, vm: u8) {
        self.float_binop(vd, vn, vm, false, 0x5e);
    }

    pub fn fabs_s(&mut self, vd: u8, vn: u8) {
        self.load_slot32(RAX, vn).expect("virtual register fits");
        self.alu_imm32(0x81, 4, RAX, 0x7fff_ffff);
        self.store_slot32(vd, RAX).expect("virtual register fits");
    }

    pub fn fabs_d(&mut self, vd: u8, vn: u8) {
        self.load_slot64(RAX, vn).expect("virtual register fits");
        self.mov_imm64(RCX, 0x7fff_ffff_ffff_ffff);
        self.alu_rm64(0x23, RAX, RCX);
        self.store_slot64(vd, RAX).expect("virtual register fits");
    }

    pub fn fneg_s(&mut self, vd: u8, vn: u8) {
        self.load_slot32(RAX, vn).expect("virtual register fits");
        self.alu_imm32(0x81, 6, RAX, 0x8000_0000);
        self.store_slot32(vd, RAX).expect("virtual register fits");
    }

    pub fn fneg_d(&mut self, vd: u8, vn: u8) {
        self.load_slot64(RAX, vn).expect("virtual register fits");
        self.mov_imm64(RCX, 0x8000_0000_0000_0000);
        self.alu_rm64(0x33, RAX, RCX);
        self.store_slot64(vd, RAX).expect("virtual register fits");
    }

    pub fn fsqrt_s(&mut self, vd: u8, vn: u8) {
        self.float_unop(vd, vn, false, 0x51);
    }

    pub fn fsqrt_d(&mut self, vd: u8, vn: u8) {
        self.float_unop(vd, vn, true, 0x51);
    }

    pub fn frintn_s(&mut self, vd: u8, vn: u8) -> AsmResult {
        self.round(vd, vn, false, 0)
    }

    pub fn frintn_d(&mut self, vd: u8, vn: u8) -> AsmResult {
        self.round(vd, vn, true, 0)
    }

    pub fn frintp_s(&mut self, vd: u8, vn: u8) -> AsmResult {
        self.round(vd, vn, false, 2)
    }

    pub fn frintp_d(&mut self, vd: u8, vn: u8) -> AsmResult {
        self.round(vd, vn, true, 2)
    }

    pub fn frintm_s(&mut self, vd: u8, vn: u8) -> AsmResult {
        self.round(vd, vn, false, 1)
    }

    pub fn frintm_d(&mut self, vd: u8, vn: u8) -> AsmResult {
        self.round(vd, vn, true, 1)
    }

    pub fn frintz_s(&mut self, vd: u8, vn: u8) -> AsmResult {
        self.round(vd, vn, false, 3)
    }

    pub fn frintz_d(&mut self, vd: u8, vn: u8) -> AsmResult {
        self.round(vd, vn, true, 3)
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
        self.load_xmm_slot(0, vn, true)
            .expect("virtual register fits");
        self.emit(&[0xf2, 0x0f, 0x5a, 0xc0]);
        self.store_xmm_slot(0, vd, false)
            .expect("virtual register fits");
    }

    pub fn fcvt_d_from_s(&mut self, vd: u8, vn: u8) {
        self.load_xmm_slot(0, vn, false)
            .expect("virtual register fits");
        self.emit(&[0xf3, 0x0f, 0x5a, 0xc0]);
        self.store_xmm_slot(0, vd, true)
            .expect("virtual register fits");
    }

    pub fn fcvt_w_from_s(&mut self, rd: u8, vn: u8, signed: bool) {
        self.fcvt_w(rd, vn, false, signed);
    }

    pub fn fcvt_w_from_d(&mut self, rd: u8, vn: u8, signed: bool) {
        self.fcvt_w(rd, vn, true, signed);
    }

    pub fn lsl_w_imm(&mut self, rd: u8, rn: u8, shift: u32) {
        self.shift_imm(rd, rn, false, 4, shift & 31);
    }

    pub fn lsr_w_imm(&mut self, rd: u8, rn: u8, shift: u32) {
        self.shift_imm(rd, rn, false, 5, shift & 31);
    }

    pub fn asr_w_imm(&mut self, rd: u8, rn: u8, shift: u32) {
        self.shift_imm(rd, rn, false, 7, shift & 31);
    }

    pub fn ubfm_w(&mut self, rd: u8, rn: u8, immr: u32, imms: u32) {
        if immr == 0 {
            self.load_slot32(RAX, rn).expect("virtual register fits");
            let mask = if imms >= 31 {
                u32::MAX
            } else {
                (1u32 << (imms + 1)) - 1
            };
            self.alu_imm32(0x81, 4, RAX, mask);
            self.store_slot32(rd, RAX).expect("virtual register fits");
        } else {
            self.lsr_w_imm(rd, rn, immr);
        }
    }

    pub fn ubfm_x(&mut self, rd: u8, rn: u8, immr: u32, imms: u32) {
        if immr == 0 {
            self.load_slot64(RAX, rn).expect("virtual register fits");
            let bits = imms + 1;
            if bits < 64 {
                self.mov_imm64(RCX, (1u64 << bits) - 1);
                self.alu_rm64(0x23, RAX, RCX);
            }
            self.store_slot64(rd, RAX).expect("virtual register fits");
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
        self.shift_imm(rd, rn, true, 4, shift);
        Ok(())
    }

    pub fn lsr_x_imm(&mut self, rd: u8, rn: u8, shift: u32) -> AsmResult {
        if shift > 63 {
            return Err(AsmError::InvalidImmediate);
        }
        self.shift_imm(rd, rn, true, 5, shift);
        Ok(())
    }

    pub fn ldr_x_imm(&mut self, rt: u8, rn: u8, offset: usize) -> AsmResult {
        self.load_slot64(RAX, rn)?;
        self.load_mem_disp64(
            RAX,
            RAX,
            i32::try_from(offset).map_err(|_| AsmError::InvalidImmediate)?,
        );
        self.store_slot64(rt, RAX)
    }

    pub fn ldr_w_imm(&mut self, rt: u8, rn: u8, offset: usize) -> AsmResult {
        self.load_slot64(RAX, rn)?;
        self.load_mem_disp32(
            RAX,
            RAX,
            i32::try_from(offset).map_err(|_| AsmError::InvalidImmediate)?,
        );
        self.store_slot32(rt, RAX)
    }

    pub fn ldr_w_unscaled_imm(&mut self, rt: u8, rn: u8, offset: u32) -> AsmResult {
        self.ldr_w_imm(rt, rn, offset as usize)
    }

    pub fn ldrb_w(&mut self, rt: u8, rn: u8) {
        self.load_mem_extend(rt, rn, 1, false);
    }

    pub fn ldrsb_w(&mut self, rt: u8, rn: u8) {
        self.load_mem_extend(rt, rn, 1, true);
    }

    pub fn ldrh_w(&mut self, rt: u8, rn: u8) {
        self.load_mem_extend(rt, rn, 2, false);
    }

    pub fn ldrsh_w(&mut self, rt: u8, rn: u8) {
        self.load_mem_extend(rt, rn, 2, true);
    }

    pub fn ldr_w(&mut self, rt: u8, rn: u8) {
        self.load_mem_extend(rt, rn, 4, false);
    }

    pub fn strb_w(&mut self, rt: u8, rn: u8) {
        self.store_mem_width(rt, rn, 1);
    }

    pub fn strh_w(&mut self, rt: u8, rn: u8) {
        self.store_mem_width(rt, rn, 2);
    }

    pub fn str_w(&mut self, rt: u8, rn: u8) {
        self.store_mem_width(rt, rn, 4);
    }

    pub fn str_x_imm(&mut self, rt: u8, rn: u8, offset: usize) -> AsmResult {
        self.load_slot64(RAX, rn)?;
        self.load_slot64(RCX, rt)?;
        self.store_mem_disp64(
            RAX,
            i32::try_from(offset).map_err(|_| AsmError::InvalidImmediate)?,
            RCX,
        );
        Ok(())
    }

    pub fn str_w_imm(&mut self, rt: u8, rn: u8, offset: usize) -> AsmResult {
        self.load_slot64(RAX, rn)?;
        self.load_slot32(RCX, rt)?;
        self.store_mem_disp32(
            RAX,
            i32::try_from(offset).map_err(|_| AsmError::InvalidImmediate)?,
            RCX,
        );
        Ok(())
    }

    pub fn str_w_unscaled_imm(&mut self, rt: u8, rn: u8, offset: u32) -> AsmResult {
        self.str_w_imm(rt, rn, offset as usize)
    }

    fn emit(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes);
        self.pending_rbit = None;
    }

    fn emit_keep_pending(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes);
    }

    fn slot_disp(reg: u8) -> AsmResult<i32> {
        if usize::from(reg) < VREGS {
            Ok(-((i32::from(reg) + 1) * SLOT_SIZE))
        } else {
            Err(AsmError::InvalidRegister)
        }
    }

    fn emit_rex(&mut self, wide: bool, reg: u8, rm: u8) {
        let mut rex = 0x40;
        if wide {
            rex |= 0x08;
        }
        if reg & 8 != 0 {
            rex |= 0x04;
        }
        if rm & 8 != 0 {
            rex |= 0x01;
        }
        if rex != 0x40 {
            self.bytes.push(rex);
        }
    }

    fn modrm_rr(&mut self, rm: u8, reg: u8) {
        self.bytes.push(0xc0 | ((reg & 7) << 3) | (rm & 7));
    }

    fn modrm_rbp_disp32(&mut self, reg: u8, disp: i32) {
        self.bytes.push(0x80 | ((reg & 7) << 3) | (RBP & 7));
        self.bytes.extend_from_slice(&disp.to_le_bytes());
    }

    fn load_slot64(&mut self, dst: u8, slot: u8) -> AsmResult {
        let disp = Self::slot_disp(slot)?;
        self.emit_rex(true, dst, RBP);
        self.bytes.push(0x8b);
        self.modrm_rbp_disp32(dst, disp);
        Ok(())
    }

    fn load_slot32(&mut self, dst: u8, slot: u8) -> AsmResult {
        let disp = Self::slot_disp(slot)?;
        self.emit_rex(false, dst, RBP);
        self.bytes.push(0x8b);
        self.modrm_rbp_disp32(dst, disp);
        Ok(())
    }

    fn store_slot64(&mut self, slot: u8, src: u8) -> AsmResult {
        let disp = Self::slot_disp(slot)?;
        self.emit_rex(true, src, RBP);
        self.bytes.push(0x89);
        self.modrm_rbp_disp32(src, disp);
        Ok(())
    }

    fn store_slot32(&mut self, slot: u8, src: u8) -> AsmResult {
        self.store_slot64(slot, src)
    }

    fn mov_imm32(&mut self, reg: u8, value: u32) {
        self.emit_rex(false, 0, reg);
        self.bytes.push(0xb8 | (reg & 7));
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn mov_imm64(&mut self, reg: u8, value: u64) {
        self.emit_rex(true, 0, reg);
        self.bytes.push(0xb8 | (reg & 7));
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn alu_rm32(&mut self, opcode: u8, dst: u8, src: u8) {
        self.emit_rex(false, dst, src);
        self.bytes.push(opcode);
        self.modrm_rr(src, dst);
    }

    fn alu_rm64(&mut self, opcode: u8, dst: u8, src: u8) {
        self.emit_rex(true, dst, src);
        self.bytes.push(opcode);
        self.modrm_rr(src, dst);
    }

    fn alu_imm32(&mut self, opcode: u8, subop: u8, reg: u8, imm: u32) {
        self.emit_rex(false, subop, reg);
        self.bytes.push(opcode);
        self.modrm_rr(reg, subop);
        self.bytes.extend_from_slice(&imm.to_le_bytes());
    }

    fn alu_imm32_wide(&mut self, opcode: u8, subop: u8, reg: u8, imm: u32) {
        self.emit_rex(true, subop, reg);
        self.bytes.push(opcode);
        self.modrm_rr(reg, subop);
        self.bytes.extend_from_slice(&imm.to_le_bytes());
    }

    fn binop_w(&mut self, rd: u8, rn: u8, rm: u8, opcode: u8) -> AsmResult {
        self.load_slot32(RAX, rn)?;
        self.load_slot32(RCX, rm)?;
        self.alu_rm32(opcode, RAX, RCX);
        self.store_slot32(rd, RAX)
    }

    fn binop_x(&mut self, rd: u8, rn: u8, rm: u8, opcode: u8) -> AsmResult {
        self.load_slot64(RAX, rn)?;
        self.load_slot64(RCX, rm)?;
        self.alu_rm64(opcode, RAX, RCX);
        self.store_slot64(rd, RAX)
    }

    fn imul_slot32(&mut self, dst: u8, slot: u8) -> AsmResult {
        let disp = Self::slot_disp(slot)?;
        self.emit_rex(false, dst, RBP);
        self.emit(&[0x0f, 0xaf]);
        self.modrm_rbp_disp32(dst, disp);
        Ok(())
    }

    fn imul_slot64(&mut self, dst: u8, slot: u8) -> AsmResult {
        let disp = Self::slot_disp(slot)?;
        self.emit_rex(true, dst, RBP);
        self.emit(&[0x0f, 0xaf]);
        self.modrm_rbp_disp32(dst, disp);
        Ok(())
    }

    fn emit_modrm_reg(&mut self, opcode: u8, subop: u8, rm: u8, wide: bool) {
        self.emit_rex(wide, subop, rm);
        self.bytes.push(opcode);
        self.modrm_rr(rm, subop);
    }

    fn shift_var(&mut self, rd: u8, rn: u8, rm: u8, wide: bool, subop: u8) {
        if wide {
            self.load_slot64(RAX, rn).expect("virtual register fits");
        } else {
            self.load_slot32(RAX, rn).expect("virtual register fits");
        }
        self.load_slot32(RCX, rm).expect("virtual register fits");
        self.emit_modrm_reg(0xd3, subop, RAX, wide);
        if wide {
            self.store_slot64(rd, RAX).expect("virtual register fits");
        } else {
            self.store_slot32(rd, RAX).expect("virtual register fits");
        }
    }

    fn shift_imm(&mut self, rd: u8, rn: u8, wide: bool, subop: u8, shift: u32) {
        if wide {
            self.load_slot64(RAX, rn).expect("virtual register fits");
        } else {
            self.load_slot32(RAX, rn).expect("virtual register fits");
        }
        self.emit_rex(wide, subop, RAX);
        self.bytes.push(0xc1);
        self.modrm_rr(RAX, subop);
        self.bytes.push(shift as u8);
        if wide {
            self.store_slot64(rd, RAX).expect("virtual register fits");
        } else {
            self.store_slot32(rd, RAX).expect("virtual register fits");
        }
    }

    fn asr_x_imm(&mut self, rd: u8, rn: u8, shift: u32) {
        self.shift_imm(rd, rn, true, 7, shift & 63);
    }

    fn clz(&mut self, rd: u8, rn: u8, wide: bool) {
        if wide {
            self.load_slot64(RAX, rn).expect("virtual register fits");
        } else {
            self.load_slot32(RAX, rn).expect("virtual register fits");
        }
        self.test_rr(RAX, RAX, wide);
        let zero_jcc = self.emit_jcc_placeholder(Cond::Eq);
        self.emit_rex(wide, RAX, RAX);
        self.emit(&[0x0f, 0xbd]);
        self.modrm_rr(RAX, RAX);
        self.alu_imm32(0x81, 6, RAX, if wide { 63 } else { 31 });
        let done_jmp = self.emit_jmp_placeholder();
        let zero_target = self.offset();
        self.mov_imm32(RAX, if wide { 64 } else { 32 });
        let done_target = self.offset();
        patch_branch(
            &mut self.bytes,
            zero_jcc,
            zero_target,
            BranchKind::BCond(Cond::Eq),
        )
        .expect("local branch fits");
        patch_branch(&mut self.bytes, done_jmp, done_target, BranchKind::B)
            .expect("local branch fits");
        if wide {
            self.store_slot64(rd, RAX).expect("virtual register fits");
        } else {
            self.store_slot32(rd, RAX).expect("virtual register fits");
        }
    }

    fn ctz(&mut self, rd: u8, rn: u8, wide: bool) {
        if wide {
            self.load_slot64(RAX, rn).expect("virtual register fits");
        } else {
            self.load_slot32(RAX, rn).expect("virtual register fits");
        }
        self.test_rr(RAX, RAX, wide);
        let zero_jcc = self.emit_jcc_placeholder(Cond::Eq);
        self.emit_rex(wide, RAX, RAX);
        self.emit(&[0x0f, 0xbc]);
        self.modrm_rr(RAX, RAX);
        let done_jmp = self.emit_jmp_placeholder();
        let zero_target = self.offset();
        self.mov_imm32(RAX, if wide { 64 } else { 32 });
        let done_target = self.offset();
        patch_branch(
            &mut self.bytes,
            zero_jcc,
            zero_target,
            BranchKind::BCond(Cond::Eq),
        )
        .expect("local branch fits");
        patch_branch(&mut self.bytes, done_jmp, done_target, BranchKind::B)
            .expect("local branch fits");
        if wide {
            self.store_slot64(rd, RAX).expect("virtual register fits");
        } else {
            self.store_slot32(rd, RAX).expect("virtual register fits");
        }
    }

    fn emit_jcc_placeholder(&mut self, cond: Cond) -> usize {
        let at = self.offset();
        self.emit(&[0x0f, 0x80 | cond as u8, 0, 0, 0, 0]);
        at
    }

    fn emit_jmp_placeholder(&mut self) -> usize {
        let at = self.offset();
        self.emit(&[0xe9, 0, 0, 0, 0]);
        at
    }

    fn test_rr(&mut self, lhs: u8, rhs: u8, wide: bool) {
        self.emit_rex(wide, lhs, rhs);
        self.bytes.push(0x85);
        self.modrm_rr(rhs, lhs);
    }

    fn test_rr32(&mut self, lhs: u8, rhs: u8) {
        self.test_rr(lhs, rhs, false);
    }

    fn test_rr64(&mut self, lhs: u8, rhs: u8) {
        self.test_rr(lhs, rhs, true);
    }

    fn movsx_slot32(&mut self, rd: u8, rn: u8, opcode2: u8) {
        let disp = Self::slot_disp(rn).expect("virtual register fits");
        self.emit_rex(false, RAX, RBP);
        self.emit(&[0x0f, opcode2]);
        self.modrm_rbp_disp32(RAX, disp);
        self.store_slot32(rd, RAX).expect("virtual register fits");
    }

    fn cmp_slot32(&mut self, lhs_reg: u8, rhs_slot: u8) -> AsmResult {
        let disp = Self::slot_disp(rhs_slot)?;
        self.emit_rex(false, lhs_reg, RBP);
        self.bytes.push(0x3b);
        self.modrm_rbp_disp32(lhs_reg, disp);
        Ok(())
    }

    fn cmp_slot64(&mut self, lhs_reg: u8, rhs_slot: u8) -> AsmResult {
        let disp = Self::slot_disp(rhs_slot)?;
        self.emit_rex(true, lhs_reg, RBP);
        self.bytes.push(0x3b);
        self.modrm_rbp_disp32(lhs_reg, disp);
        Ok(())
    }

    fn load_xmm_slot(&mut self, xmm: u8, slot: u8, f64: bool) -> AsmResult {
        let disp = Self::slot_disp(slot)?;
        self.bytes.push(if f64 { 0xf2 } else { 0xf3 });
        self.emit_rex(false, xmm, RBP);
        self.emit_keep_pending(&[0x0f, 0x10]);
        self.modrm_rbp_disp32(xmm, disp);
        Ok(())
    }

    fn store_xmm_slot(&mut self, xmm: u8, slot: u8, f64: bool) -> AsmResult {
        let disp = Self::slot_disp(slot)?;
        self.bytes.push(if f64 { 0xf2 } else { 0xf3 });
        self.emit_rex(false, xmm, RBP);
        self.emit_keep_pending(&[0x0f, 0x11]);
        self.modrm_rbp_disp32(xmm, disp);
        Ok(())
    }

    fn float_binop(&mut self, vd: u8, vn: u8, vm: u8, f64: bool, opcode: u8) {
        self.load_xmm_slot(0, vn, f64)
            .expect("virtual register fits");
        let disp = Self::slot_disp(vm).expect("virtual register fits");
        self.bytes.push(if f64 { 0xf2 } else { 0xf3 });
        self.emit_rex(false, 0, RBP);
        self.emit_keep_pending(&[0x0f, opcode]);
        self.modrm_rbp_disp32(0, disp);
        self.store_xmm_slot(0, vd, f64)
            .expect("virtual register fits");
    }

    fn float_unop(&mut self, vd: u8, vn: u8, f64: bool, opcode: u8) {
        let disp = Self::slot_disp(vn).expect("virtual register fits");
        self.bytes.push(if f64 { 0xf2 } else { 0xf3 });
        self.emit_rex(false, 0, RBP);
        self.emit_keep_pending(&[0x0f, opcode]);
        self.modrm_rbp_disp32(0, disp);
        self.store_xmm_slot(0, vd, f64)
            .expect("virtual register fits");
    }

    fn round(&mut self, vd: u8, vn: u8, f64: bool, mode: u8) -> AsmResult {
        if !sse41_rounding_supported() {
            return Err(AsmError::UnsupportedFeature);
        }
        let disp = Self::slot_disp(vn)?;
        self.emit(&[0x66, 0x0f, 0x3a, if f64 { 0x0b } else { 0x0a }]);
        self.modrm_rbp_disp32(0, disp);
        self.bytes.push(mode);
        self.store_xmm_slot(0, vd, f64)
    }

    fn cvt_int_to_float(&mut self, vd: u8, rn: u8, to_f64: bool, from_i64: bool, signed: bool) {
        if from_i64 {
            self.load_slot64(RAX, rn).expect("virtual register fits");
            if signed {
                self.cvt_i64_reg_to_float(to_f64);
            } else {
                self.cvt_u64_reg_to_float(to_f64);
            }
        } else if signed {
            let disp = Self::slot_disp(rn).expect("virtual register fits");
            self.emit_rex(true, RAX, RBP);
            self.emit(&[0x63]);
            self.modrm_rbp_disp32(RAX, disp);
            self.cvt_i64_reg_to_float(to_f64);
        } else {
            self.load_slot32(RAX, rn).expect("virtual register fits");
            self.cvt_i64_reg_to_float(to_f64);
        }
        self.store_xmm_slot(0, vd, to_f64)
            .expect("virtual register fits");
    }

    fn cvt_i64_reg_to_float(&mut self, to_f64: bool) {
        self.bytes.push(if to_f64 { 0xf2 } else { 0xf3 });
        self.emit(&[0x48, 0x0f, 0x2a, 0xc0]);
    }

    fn cvt_u64_reg_to_float(&mut self, to_f64: bool) {
        self.test_rr64(RAX, RAX);
        let nonnegative = self.emit_jcc_placeholder(Cond::Ge);

        self.emit_rex(true, RCX, RAX);
        self.bytes.push(0x8b);
        self.modrm_rr(RAX, RCX);
        self.emit_rex(true, 5, RCX);
        self.bytes.push(0xc1);
        self.modrm_rr(RCX, 5);
        self.bytes.push(1);
        self.alu_imm32_wide(0x81, 4, RAX, 1);
        self.alu_rm64(0x0b, RAX, RCX);
        self.cvt_i64_reg_to_float(to_f64);
        self.bytes.push(if to_f64 { 0xf2 } else { 0xf3 });
        self.emit(&[0x0f, 0x58, 0xc0]);
        let done = self.emit_jmp_placeholder();

        let nonnegative_target = self.offset();
        self.cvt_i64_reg_to_float(to_f64);
        let done_target = self.offset();
        patch_branch(
            &mut self.bytes,
            nonnegative,
            nonnegative_target,
            BranchKind::BCond(Cond::Ge),
        )
        .expect("local branch fits");
        patch_branch(&mut self.bytes, done, done_target, BranchKind::B).expect("local branch fits");
    }

    fn fcvt_w(&mut self, rd: u8, vn: u8, f64: bool, signed: bool) {
        let disp = Self::slot_disp(vn).expect("virtual register fits");
        self.bytes.push(if f64 { 0xf2 } else { 0xf3 });
        self.emit_rex(!signed, RAX, RBP);
        self.emit_keep_pending(&[0x0f, 0x2c]);
        self.modrm_rbp_disp32(RAX, disp);
        self.store_slot32(rd, RAX).expect("virtual register fits");
    }

    fn float_cset(&mut self, rd: u8, cond: Cond, width: FloatWidth, lhs: u8, rhs: u8) -> AsmResult {
        let f64 = width == FloatWidth::F64;
        self.load_xmm_slot(0, lhs, f64)?;
        let disp = Self::slot_disp(rhs)?;
        if f64 {
            self.emit(&[0x66, 0x0f, 0x2e]);
        } else {
            self.emit(&[0x0f, 0x2e]);
        }
        self.modrm_rbp_disp32(0, disp);
        match cond {
            Cond::Eq => {
                self.emit(&[0x0f, 0x94, 0xc0, 0x0f, 0x9b, 0xc1, 0x20, 0xc8]);
            }
            Cond::Ne => {
                self.emit(&[0x0f, 0x95, 0xc0, 0x0f, 0x9a, 0xc1, 0x08, 0xc8]);
            }
            Cond::Lo => {
                self.emit(&[0x0f, 0x92, 0xc0, 0x0f, 0x9b, 0xc1, 0x20, 0xc8]);
            }
            Cond::Gt => {
                self.emit(&[0x0f, 0x97, 0xc0]);
            }
            Cond::Ls => {
                self.emit(&[0x0f, 0x96, 0xc0, 0x0f, 0x9b, 0xc1, 0x20, 0xc8]);
            }
            Cond::Ge => {
                self.emit(&[0x0f, 0x93, 0xc0]);
            }
            _ => return Err(AsmError::InvalidImmediate),
        }
        self.emit(&[0x0f, 0xb6, 0xc0]);
        self.store_slot32(rd, RAX)
    }

    fn load_mem_disp64(&mut self, dst: u8, base: u8, disp: i32) {
        self.emit_rex(true, dst, base);
        self.bytes.push(0x8b);
        self.modrm_mem_disp32(dst, base, disp);
    }

    fn load_mem_disp32(&mut self, dst: u8, base: u8, disp: i32) {
        self.emit_rex(false, dst, base);
        self.bytes.push(0x8b);
        self.modrm_mem_disp32(dst, base, disp);
    }

    fn store_mem_disp64(&mut self, base: u8, disp: i32, src: u8) {
        self.emit_rex(true, src, base);
        self.bytes.push(0x89);
        self.modrm_mem_disp32(src, base, disp);
    }

    fn store_frame_disp64(&mut self, disp: i32, src: u8) {
        self.emit_rex(true, src, RBP);
        self.bytes.push(0x89);
        self.modrm_rbp_disp32(src, disp);
    }

    fn store_mem_disp32(&mut self, base: u8, disp: i32, src: u8) {
        self.emit_rex(false, src, base);
        self.bytes.push(0x89);
        self.modrm_mem_disp32(src, base, disp);
    }

    fn modrm_mem_disp32(&mut self, reg: u8, base: u8, disp: i32) {
        self.bytes.push(0x80 | ((reg & 7) << 3) | (base & 7));
        if base & 7 == 4 {
            self.bytes.push(0x24);
        }
        self.bytes.extend_from_slice(&disp.to_le_bytes());
    }

    fn load_mem_extend(&mut self, rt: u8, rn: u8, width: u32, signed: bool) {
        self.load_slot64(RCX, rn).expect("virtual register fits");
        match (width, signed) {
            (1, false) => self.emit(&[0x0f, 0xb6]),
            (1, true) => self.emit(&[0x0f, 0xbe]),
            (2, false) => self.emit(&[0x0f, 0xb7]),
            (2, true) => self.emit(&[0x0f, 0xbf]),
            (4, _) => {
                self.emit_rex(false, RAX, RCX);
                self.bytes.push(0x8b);
                self.modrm_mem_disp32(RAX, RCX, 0);
                self.store_slot32(rt, RAX).expect("virtual register fits");
                return;
            }
            _ => unreachable!("invalid load width"),
        }
        self.modrm_mem_disp32(RAX, RCX, 0);
        self.store_slot32(rt, RAX).expect("virtual register fits");
    }

    fn store_mem_width(&mut self, rt: u8, rn: u8, width: u32) {
        self.load_slot64(RCX, rn).expect("virtual register fits");
        self.load_slot32(RAX, rt).expect("virtual register fits");
        match width {
            1 => {
                self.emit_rex(false, RAX, RCX);
                self.bytes.push(0x88);
            }
            2 => {
                self.bytes.push(0x66);
                self.emit_rex(false, RAX, RCX);
                self.bytes.push(0x89);
            }
            4 => {
                self.emit_rex(false, RAX, RCX);
                self.bytes.push(0x89);
            }
            _ => unreachable!("invalid store width"),
        }
        self.modrm_mem_disp32(RAX, RCX, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_info_matches_current_arch_gate() {
        assert_eq!(
            target_info().baseline_supported,
            cfg!(all(
                any(target_os = "macos", target_os = "linux"),
                target_arch = "x86_64"
            ))
        );
    }

    #[test]
    fn emits_prologue_once_from_aarch64_style_save() {
        let mut asm = X64BaselineMasm::with_capacity(64);
        asm.stp_pre(29, 30);
        asm.stp_pre(19, 20);
        assert_eq!(&asm.into_bytes()[..4], &[0x55, 0x48, 0x89, 0xe5]);
    }

    #[test]
    fn patches_unconditional_branch_forward() {
        let mut asm = X64BaselineMasm::with_capacity(16);
        let at = asm.branch_placeholder(BranchKind::B).unwrap();
        asm.mov_imm_u32(0, 1);
        let target = asm.offset();
        patch_branch(asm.as_mut_bytes(), at, target, BranchKind::B).unwrap();
        assert_eq!(asm.as_mut_bytes()[at], 0xe9);
        assert_eq!(
            i32::from_le_bytes(asm.as_mut_bytes()[at + 1..at + 5].try_into().unwrap()),
            (target - (at + 5)) as i32
        );
    }

    #[test]
    fn emits_ret_encoding() {
        let mut asm = X64BaselineMasm::with_capacity(1);
        asm.ret();
        assert_eq!(asm.into_bytes(), vec![0xc3]);
    }

    #[test]
    fn emits_integer_add_through_virtual_slots() {
        let mut asm = X64BaselineMasm::with_capacity(64);
        asm.add_w(2, 0, 1);
        let bytes = asm.into_bytes();
        assert!(bytes.windows(2).any(|window| window == [0x03, 0xc1]));
    }

    #[test]
    fn emits_indirect_helper_call() {
        let mut asm = X64BaselineMasm::with_capacity(128);
        asm.blr_x(16);
        assert!(asm
            .into_bytes()
            .windows(3)
            .any(|window| window == [0x41, 0xff, 0xd3]));
    }

    #[test]
    fn emits_load_store_opcodes() {
        let mut asm = X64BaselineMasm::with_capacity(128);
        asm.ldr_w_imm(1, 0, 12).unwrap();
        asm.str_w_imm(1, 0, 16).unwrap();
        let bytes = asm.into_bytes();
        assert!(bytes.contains(&0x8b));
        assert!(bytes.contains(&0x89));
    }

    #[test]
    fn emits_float_add_opcode() {
        let mut asm = X64BaselineMasm::with_capacity(64);
        asm.fadd_s(2, 0, 1);
        assert!(asm
            .into_bytes()
            .windows(3)
            .any(|window| window == [0xf3, 0x0f, 0x58]));
    }

    #[test]
    fn sse41_rounding_is_cpu_feature_gated() {
        let mut asm = X64BaselineMasm::with_capacity(64);
        let result = asm.frintp_s(2, 1);
        if sse41_rounding_supported() {
            assert!(result.is_ok());
            assert!(asm
                .into_bytes()
                .windows(4)
                .any(|window| window == [0x66, 0x0f, 0x3a, 0x0a]));
        } else {
            assert_eq!(result, Err(crate::masm::AsmError::UnsupportedFeature));
            assert!(asm.into_bytes().is_empty());
        }
    }

    #[test]
    fn signed_i32_to_float_sign_extends_before_64_bit_conversion() {
        let mut asm = X64BaselineMasm::with_capacity(64);
        asm.cvtf_s_from_w(0, 1, true);
        let bytes = asm.into_bytes();
        assert!(
            bytes.windows(2).any(|window| window == [0x48, 0x63]),
            "signed i32 conversion must use movsxd r64, r/m32 before cvtsi2ss: {bytes:02x?}"
        );
        assert!(
            bytes
                .windows(5)
                .any(|window| window == [0xf3, 0x48, 0x0f, 0x2a, 0xc0]),
            "signed i32 conversion must convert the sign-extended 64-bit value: {bytes:02x?}"
        );
    }

    #[test]
    fn unsigned_i64_to_float_handles_high_bit_values() {
        let mut asm = X64BaselineMasm::with_capacity(128);
        asm.cvtf_d_from_x(0, 1, false);
        let bytes = asm.into_bytes();
        assert!(
            bytes.windows(2).any(|window| window == [0x0f, 0x8d]),
            "unsigned i64 conversion must branch around the high-bit repair path: {bytes:02x?}"
        );
        assert!(
            bytes
                .windows(4)
                .any(|window| window == [0xf2, 0x0f, 0x58, 0xc0]),
            "unsigned i64 conversion must double the repaired half value: {bytes:02x?}"
        );
    }
}
