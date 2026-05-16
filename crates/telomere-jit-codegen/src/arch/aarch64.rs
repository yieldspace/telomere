use crate::masm::{AsmError, AsmResult};

mod enc {
    pub(super) const B: u32 = 0x1400_0000;
    pub(super) const B_COND: u32 = 0x5400_0000;
    pub(super) const CBNZ_X: u32 = 0xb500_0000;
    pub(super) const CBNZ_W: u32 = 0x3500_0000;
    pub(super) const CBZ_W: u32 = 0x3400_0000;

    pub(super) const RET: u32 = 0xd65f_03c0;
    pub(super) const MOV_X_REG: u32 = 0xaa00_03e0;
    pub(super) const MOV_W_REG: u32 = 0x2a00_03e0;
    pub(super) const MOVZ_W: u32 = 0x5280_0000;
    pub(super) const MOVK_W: u32 = 0x7280_0000;
    pub(super) const MOVZ_X: u32 = 0xd280_0000;
    pub(super) const MOVK_X: u32 = 0xf280_0000;
    pub(super) const BLR: u32 = 0xd63f_0000;
    pub(super) const STP_PRE_X_SP: u32 = 0xa980_0000;
    pub(super) const LDP_POST_X_SP: u32 = 0xa8c0_0000;

    pub(super) const ADD_W_IMM: u32 = 0x1100_0000;
    pub(super) const ADD_X_IMM: u32 = 0x9100_0000;
    pub(super) const SUB_W_IMM: u32 = 0x5100_0000;
    pub(super) const ADD_W_REG: u32 = 0x0b00_0000;
    pub(super) const ADD_X_REG: u32 = 0x8b00_0000;
    pub(super) const SUB_W_REG: u32 = 0x4b00_0000;
    pub(super) const SUB_X_REG: u32 = 0xcb00_0000;
    pub(super) const MUL_W: u32 = 0x1b00_7c00;
    pub(super) const MUL_X: u32 = 0x9b00_7c00;
    pub(super) const UDIV_W: u32 = 0x1ac0_0800;
    pub(super) const SDIV_W: u32 = 0x1ac0_0c00;
    pub(super) const UDIV_X: u32 = 0x9ac0_0800;
    pub(super) const SDIV_X: u32 = 0x9ac0_0c00;
    pub(super) const MSUB_W: u32 = 0x1b00_8000;
    pub(super) const MSUB_X: u32 = 0x9b00_8000;
    pub(super) const NEG_W: u32 = 0x4b00_03e0;
    pub(super) const NEG_X: u32 = 0xcb00_03e0;
    pub(super) const AND_W_REG: u32 = 0x0a00_0000;
    pub(super) const AND_X_REG: u32 = 0x8a00_0000;
    pub(super) const ORR_W_REG: u32 = 0x2a00_0000;
    pub(super) const ORR_X_REG: u32 = 0xaa00_0000;
    pub(super) const EOR_W_REG: u32 = 0x4a00_0000;
    pub(super) const EOR_X_REG: u32 = 0xca00_0000;
    pub(super) const LSLV_W: u32 = 0x1ac0_2000;
    pub(super) const LSLV_X: u32 = 0x9ac0_2000;
    pub(super) const LSRV_W: u32 = 0x1ac0_2400;
    pub(super) const LSRV_X: u32 = 0x9ac0_2400;
    pub(super) const ASRV_W: u32 = 0x1ac0_2800;
    pub(super) const ASRV_X: u32 = 0x9ac0_2800;
    pub(super) const RORV_W: u32 = 0x1ac0_2c00;
    pub(super) const RORV_X: u32 = 0x9ac0_2c00;
    pub(super) const RBIT_W: u32 = 0x5ac0_0000;
    pub(super) const RBIT_X: u32 = 0xdac0_0000;
    pub(super) const CLZ_W: u32 = 0x5ac0_1000;
    pub(super) const CLZ_X: u32 = 0xdac0_1000;
    pub(super) const SXTH_W: u32 = 0x1300_3c00;
    pub(super) const SXTB_W: u32 = 0x1300_1c00;
    pub(super) const CMP_W_REG: u32 = 0x6b00_001f;
    pub(super) const CMP_X_REG: u32 = 0xeb00_001f;
    pub(super) const CMP_W_IMM: u32 = 0x7100_001f;
    pub(super) const CSET_W: u32 = 0x1a80_0400;
    pub(super) const CSEL_W: u32 = 0x1a80_0000;

    pub(super) const FMOV_S_FROM_W: u32 = 0x1e27_0000;
    pub(super) const FMOV_W_FROM_S: u32 = 0x1e26_0000;
    pub(super) const FMOV_D_FROM_X: u32 = 0x9e67_0000;
    pub(super) const FMOV_X_FROM_D: u32 = 0x9e66_0000;
    pub(super) const FCMP_S: u32 = 0x1e20_2000;
    pub(super) const FCMP_D: u32 = 0x1e60_2000;
    pub(super) const FADD_D: u32 = 0x1e60_2800;
    pub(super) const FADD_S: u32 = 0x1e20_2800;
    pub(super) const FSUB_D: u32 = 0x1e60_3800;
    pub(super) const FSUB_S: u32 = 0x1e20_3800;
    pub(super) const FMUL_D: u32 = 0x1e60_0800;
    pub(super) const FMUL_S: u32 = 0x1e20_0800;
    pub(super) const FDIV_D: u32 = 0x1e60_1800;
    pub(super) const FDIV_S: u32 = 0x1e20_1800;
    pub(super) const FABS_S: u32 = 0x1e20_c000;
    pub(super) const FABS_D: u32 = 0x1e60_c000;
    pub(super) const FNEG_S: u32 = 0x1e21_4000;
    pub(super) const FNEG_D: u32 = 0x1e61_4000;
    pub(super) const FSQRT_S: u32 = 0x1e21_c000;
    pub(super) const FSQRT_D: u32 = 0x1e61_c000;
    pub(super) const FRINTN_S: u32 = 0x1e24_4000;
    pub(super) const FRINTN_D: u32 = 0x1e64_4000;
    pub(super) const FRINTP_S: u32 = 0x1e24_c000;
    pub(super) const FRINTP_D: u32 = 0x1e64_c000;
    pub(super) const FRINTM_S: u32 = 0x1e25_4000;
    pub(super) const FRINTM_D: u32 = 0x1e65_4000;
    pub(super) const FRINTZ_S: u32 = 0x1e25_c000;
    pub(super) const FRINTZ_D: u32 = 0x1e65_c000;
    pub(super) const SCVTF_D_FROM_W: u32 = 0x1e62_0000;
    pub(super) const UCVTF_D_FROM_W: u32 = 0x1e63_0000;
    pub(super) const SCVTF_D_FROM_X: u32 = 0x9e62_0000;
    pub(super) const UCVTF_D_FROM_X: u32 = 0x9e63_0000;
    pub(super) const FCVTZS_W_FROM_S: u32 = 0x1e38_0000;
    pub(super) const FCVTZU_W_FROM_S: u32 = 0x1e39_0000;
    pub(super) const FCVTZS_W_FROM_D: u32 = 0x1e78_0000;
    pub(super) const FCVTZU_W_FROM_D: u32 = 0x1e79_0000;

    pub(super) const UBFM_W: u32 = 0x5300_0000;
    pub(super) const UBFM_X: u32 = 0xd340_0000;
    pub(super) const SBFM_W: u32 = 0x1300_0000;
    pub(super) const SBFM_X: u32 = 0x9340_0000;
    pub(super) const LDR_X_UNSIGNED_IMM: u32 = 0xf940_0000;
    pub(super) const LDR_W_UNSIGNED_IMM: u32 = 0xb940_0000;
    pub(super) const LDR_W_UNSCALED_IMM: u32 = 0xb840_0000;
    pub(super) const LDRB_W_UNSIGNED: u32 = 0x3940_0000;
    pub(super) const LDRSB_W_UNSIGNED: u32 = 0x39c0_0000;
    pub(super) const LDRH_W_UNSIGNED: u32 = 0x7940_0000;
    pub(super) const LDRSH_W_UNSIGNED: u32 = 0x79c0_0000;
    pub(super) const STRB_W_UNSIGNED: u32 = 0x3900_0000;
    pub(super) const STRH_W_UNSIGNED: u32 = 0x7900_0000;
    pub(super) const STR_X_UNSIGNED_IMM: u32 = 0xf900_0000;
    pub(super) const STR_W_UNSIGNED: u32 = 0xb900_0000;
    pub(super) const STR_W_UNSCALED_IMM: u32 = 0xb800_0000;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reg5(u8);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct XReg(Reg5);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WReg(Reg5);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VReg(Reg5);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XOrSp {
    X(XReg),
    Sp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XOrZr {
    X(XReg),
    Zr,
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cond {
    Eq = 0,
    Ne = 1,
    Hs = 2,
    Lo = 3,
    Hi = 8,
    Ls = 9,
    Ge = 10,
    Lt = 11,
    Gt = 12,
    Le = 13,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BranchKind {
    B,
    BCond(Cond),
    CbnzX(u8),
    CbnzW(u8),
    CbzW(u8),
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

impl BranchKind {
    pub fn placeholder(self) -> AsmResult<u32> {
        Ok(match self {
            Self::B => enc::B,
            Self::BCond(cond) => enc::B_COND | cond as u32,
            Self::CbnzX(rt) => enc::CBNZ_X | Reg5::new(rt)?.bits(),
            Self::CbnzW(rt) => enc::CBNZ_W | Reg5::new(rt)?.bits(),
            Self::CbzW(rt) => enc::CBZ_W | Reg5::new(rt)?.bits(),
        })
    }
}

pub fn patch_branch(bytes: &mut [u8], at: usize, target: usize, kind: BranchKind) -> AsmResult {
    let delta = target as isize - at as isize;
    if delta % 4 != 0 {
        return Err(AsmError::InvalidImmediate);
    }
    let words = delta / 4;
    let insn = match kind {
        BranchKind::B => {
            if !(-(1 << 25)..(1 << 25)).contains(&words) {
                return Err(AsmError::BranchOutOfRange);
            }
            enc::B | ((words as i32 as u32) & 0x03ff_ffff)
        }
        BranchKind::BCond(cond) => {
            if !(-(1 << 18)..(1 << 18)).contains(&words) {
                return Err(AsmError::BranchOutOfRange);
            }
            enc::B_COND | (((words as i32 as u32) & 0x7ffff) << 5) | cond as u32
        }
        BranchKind::CbnzX(rt) => {
            if !(-(1 << 18)..(1 << 18)).contains(&words) {
                return Err(AsmError::BranchOutOfRange);
            }
            enc::CBNZ_X | (((words as i32 as u32) & 0x7ffff) << 5) | Reg5::new(rt)?.bits()
        }
        BranchKind::CbnzW(rt) => {
            if !(-(1 << 18)..(1 << 18)).contains(&words) {
                return Err(AsmError::BranchOutOfRange);
            }
            enc::CBNZ_W | (((words as i32 as u32) & 0x7ffff) << 5) | Reg5::new(rt)?.bits()
        }
        BranchKind::CbzW(rt) => {
            if !(-(1 << 18)..(1 << 18)).contains(&words) {
                return Err(AsmError::BranchOutOfRange);
            }
            enc::CBZ_W | (((words as i32 as u32) & 0x7ffff) << 5) | Reg5::new(rt)?.bits()
        }
    };
    let slot = bytes
        .get_mut(at..at + 4)
        .ok_or(AsmError::InvalidImmediate)?;
    slot.copy_from_slice(&insn.to_le_bytes());
    Ok(())
}

impl Reg5 {
    pub const fn new_unchecked(n: u8) -> Self {
        Self(n)
    }

    pub fn new(n: u8) -> AsmResult<Self> {
        if n < 32 {
            Ok(Self(n))
        } else {
            Err(AsmError::InvalidRegister)
        }
    }

    pub const fn bits(self) -> u32 {
        self.0 as u32
    }
}

impl XReg {
    pub const fn new_unchecked(n: u8) -> Self {
        Self(Reg5::new_unchecked(n))
    }

    pub fn new(n: u8) -> AsmResult<Self> {
        Ok(Self(Reg5::new(n)?))
    }

    pub const fn bits(self) -> u32 {
        self.0.bits()
    }
}

impl WReg {
    pub const fn new_unchecked(n: u8) -> Self {
        Self(Reg5::new_unchecked(n))
    }

    pub fn new(n: u8) -> AsmResult<Self> {
        Ok(Self(Reg5::new(n)?))
    }

    pub const fn bits(self) -> u32 {
        self.0.bits()
    }
}

impl VReg {
    pub const fn new_unchecked(n: u8) -> Self {
        Self(Reg5::new_unchecked(n))
    }

    pub fn new(n: u8) -> AsmResult<Self> {
        Ok(Self(Reg5::new(n)?))
    }

    pub const fn bits(self) -> u32 {
        self.0.bits()
    }
}

impl XOrSp {
    const fn bits(self) -> u32 {
        match self {
            Self::X(reg) => reg.bits(),
            Self::Sp => 31,
        }
    }
}

#[derive(Debug, Clone)]
pub struct A64Masm {
    bytes: Vec<u8>,
}

impl A64Masm {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            bytes: Vec::with_capacity(capacity),
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

    pub fn insn(&mut self, insn: u32) {
        self.bytes.extend_from_slice(&insn.to_le_bytes());
    }

    pub fn ret(&mut self) {
        self.insn(enc::RET);
    }

    pub fn branch_placeholder(&mut self, kind: BranchKind) -> AsmResult<usize> {
        let at = self.offset();
        self.insn(kind.placeholder()?);
        Ok(at)
    }

    pub fn mov_x(&mut self, rd: XReg, rn: XReg) {
        self.insn(enc::MOV_X_REG | (rn.bits() << 16) | rd.bits());
    }

    pub fn mov_x_from_sp(&mut self, rd: XReg) {
        self.add_x_imm_sp(rd, XOrSp::Sp, 0)
            .expect("zero add immediate is always valid");
    }

    pub fn mov_w(&mut self, rd: WReg, rn: WReg) {
        self.insn(enc::MOV_W_REG | (rn.bits() << 16) | rd.bits());
    }

    pub fn mov_imm_u32(&mut self, rd: WReg, value: u32) {
        self.insn(enc::MOVZ_W | ((value & 0xffff) << 5) | rd.bits());
        let hi = (value >> 16) & 0xffff;
        if hi != 0 {
            self.insn(enc::MOVK_W | (1 << 21) | (hi << 5) | rd.bits());
        }
    }

    pub fn mov_imm_u64(&mut self, rd: XReg, value: u64) {
        self.insn(enc::MOVZ_X | (((value & 0xffff) as u32) << 5) | rd.bits());
        for hw in 1..4 {
            let part = ((value >> (hw * 16)) & 0xffff) as u32;
            if part != 0 {
                self.insn(enc::MOVK_X | ((hw as u32) << 21) | (part << 5) | rd.bits());
            }
        }
    }

    pub fn blr_x(&mut self, rn: XReg) {
        self.insn(enc::BLR | (rn.bits() << 5));
    }

    pub fn stp_pre_x_sp(&mut self, rt: XReg, rt2: XReg) {
        self.insn(enc::STP_PRE_X_SP | (0x7e << 15) | (rt2.bits() << 10) | (31 << 5) | rt.bits());
    }

    pub fn ldp_post_x_sp(&mut self, rt: XReg, rt2: XReg) {
        self.insn(enc::LDP_POST_X_SP | (2 << 15) | (rt2.bits() << 10) | (31 << 5) | rt.bits());
    }

    pub fn add_w_imm(&mut self, rd: WReg, rn: WReg, imm: u32) -> AsmResult {
        if imm > 4095 {
            return Err(AsmError::InvalidImmediate);
        }
        self.insn(enc::ADD_W_IMM | (imm << 10) | (rn.bits() << 5) | rd.bits());
        Ok(())
    }

    pub fn add_x_imm(&mut self, rd: XReg, rn: XReg, imm: u32) -> AsmResult {
        self.add_x_imm_sp(rd, XOrSp::X(rn), imm)
    }

    pub fn add_x_imm_sp(&mut self, rd: XReg, rn: XOrSp, imm: u32) -> AsmResult {
        if imm > 4095 {
            return Err(AsmError::InvalidImmediate);
        }
        self.insn(enc::ADD_X_IMM | (imm << 10) | (rn.bits() << 5) | rd.bits());
        Ok(())
    }

    pub fn sub_w_imm(&mut self, rd: WReg, rn: WReg, imm: u32) -> AsmResult {
        if imm > 4095 {
            return Err(AsmError::InvalidImmediate);
        }
        self.insn(enc::SUB_W_IMM | (imm << 10) | (rn.bits() << 5) | rd.bits());
        Ok(())
    }

    pub fn add_w(&mut self, rd: WReg, rn: WReg, rm: WReg) {
        self.emit_r3(enc::ADD_W_REG, rd.bits(), rn.bits(), rm.bits());
    }

    pub fn add_x(&mut self, rd: XReg, rn: XReg, rm: XReg) {
        self.emit_r3(enc::ADD_X_REG, rd.bits(), rn.bits(), rm.bits());
    }

    pub fn sub_w(&mut self, rd: WReg, rn: WReg, rm: WReg) {
        self.emit_r3(enc::SUB_W_REG, rd.bits(), rn.bits(), rm.bits());
    }

    pub fn sub_x(&mut self, rd: XReg, rn: XReg, rm: XReg) {
        self.emit_r3(enc::SUB_X_REG, rd.bits(), rn.bits(), rm.bits());
    }

    pub fn mul_w(&mut self, rd: WReg, rn: WReg, rm: WReg) {
        self.emit_r3(enc::MUL_W, rd.bits(), rn.bits(), rm.bits());
    }

    pub fn mul_x(&mut self, rd: XReg, rn: XReg, rm: XReg) {
        self.emit_r3(enc::MUL_X, rd.bits(), rn.bits(), rm.bits());
    }

    pub fn udiv_w(&mut self, rd: WReg, rn: WReg, rm: WReg) {
        self.emit_r3(enc::UDIV_W, rd.bits(), rn.bits(), rm.bits());
    }

    pub fn sdiv_w(&mut self, rd: WReg, rn: WReg, rm: WReg) {
        self.emit_r3(enc::SDIV_W, rd.bits(), rn.bits(), rm.bits());
    }

    pub fn udiv_x(&mut self, rd: XReg, rn: XReg, rm: XReg) {
        self.emit_r3(enc::UDIV_X, rd.bits(), rn.bits(), rm.bits());
    }

    pub fn sdiv_x(&mut self, rd: XReg, rn: XReg, rm: XReg) {
        self.emit_r3(enc::SDIV_X, rd.bits(), rn.bits(), rm.bits());
    }

    pub fn msub_w(&mut self, rd: WReg, rn: WReg, rm: WReg, ra: WReg) {
        self.insn(
            enc::MSUB_W | (rm.bits() << 16) | (ra.bits() << 10) | (rn.bits() << 5) | rd.bits(),
        );
    }

    pub fn msub_x(&mut self, rd: XReg, rn: XReg, rm: XReg, ra: XReg) {
        self.insn(
            enc::MSUB_X | (rm.bits() << 16) | (ra.bits() << 10) | (rn.bits() << 5) | rd.bits(),
        );
    }

    pub fn neg_w(&mut self, rd: WReg, rm: WReg) {
        self.insn(enc::NEG_W | (rm.bits() << 16) | rd.bits());
    }

    pub fn neg_x(&mut self, rd: XReg, rm: XReg) {
        self.insn(enc::NEG_X | (rm.bits() << 16) | rd.bits());
    }

    pub fn and_w(&mut self, rd: WReg, rn: WReg, rm: WReg) {
        self.emit_r3(enc::AND_W_REG, rd.bits(), rn.bits(), rm.bits());
    }

    pub fn and_x(&mut self, rd: XReg, rn: XReg, rm: XReg) {
        self.emit_r3(enc::AND_X_REG, rd.bits(), rn.bits(), rm.bits());
    }

    pub fn orr_w(&mut self, rd: WReg, rn: WReg, rm: WReg) {
        self.emit_r3(enc::ORR_W_REG, rd.bits(), rn.bits(), rm.bits());
    }

    pub fn orr_x(&mut self, rd: XReg, rn: XReg, rm: XReg) {
        self.emit_r3(enc::ORR_X_REG, rd.bits(), rn.bits(), rm.bits());
    }

    pub fn eor_w(&mut self, rd: WReg, rn: WReg, rm: WReg) {
        self.emit_r3(enc::EOR_W_REG, rd.bits(), rn.bits(), rm.bits());
    }

    pub fn eor_x(&mut self, rd: XReg, rn: XReg, rm: XReg) {
        self.emit_r3(enc::EOR_X_REG, rd.bits(), rn.bits(), rm.bits());
    }

    pub fn lslv_w(&mut self, rd: WReg, rn: WReg, rm: WReg) {
        self.emit_r3(enc::LSLV_W, rd.bits(), rn.bits(), rm.bits());
    }

    pub fn lslv_x(&mut self, rd: XReg, rn: XReg, rm: XReg) {
        self.emit_r3(enc::LSLV_X, rd.bits(), rn.bits(), rm.bits());
    }

    pub fn lsrv_w(&mut self, rd: WReg, rn: WReg, rm: WReg) {
        self.emit_r3(enc::LSRV_W, rd.bits(), rn.bits(), rm.bits());
    }

    pub fn lsrv_x(&mut self, rd: XReg, rn: XReg, rm: XReg) {
        self.emit_r3(enc::LSRV_X, rd.bits(), rn.bits(), rm.bits());
    }

    pub fn asrv_w(&mut self, rd: WReg, rn: WReg, rm: WReg) {
        self.emit_r3(enc::ASRV_W, rd.bits(), rn.bits(), rm.bits());
    }

    pub fn asrv_x(&mut self, rd: XReg, rn: XReg, rm: XReg) {
        self.emit_r3(enc::ASRV_X, rd.bits(), rn.bits(), rm.bits());
    }

    pub fn rorv_w(&mut self, rd: WReg, rn: WReg, rm: WReg) {
        self.emit_r3(enc::RORV_W, rd.bits(), rn.bits(), rm.bits());
    }

    pub fn rorv_x(&mut self, rd: XReg, rn: XReg, rm: XReg) {
        self.emit_r3(enc::RORV_X, rd.bits(), rn.bits(), rm.bits());
    }

    pub fn rbit_w(&mut self, rd: WReg, rn: WReg) {
        self.insn(enc::RBIT_W | (rn.bits() << 5) | rd.bits());
    }

    pub fn rbit_x(&mut self, rd: XReg, rn: XReg) {
        self.insn(enc::RBIT_X | (rn.bits() << 5) | rd.bits());
    }

    pub fn clz_w(&mut self, rd: WReg, rn: WReg) {
        self.insn(enc::CLZ_W | (rn.bits() << 5) | rd.bits());
    }

    pub fn clz_x(&mut self, rd: XReg, rn: XReg) {
        self.insn(enc::CLZ_X | (rn.bits() << 5) | rd.bits());
    }

    pub fn sxth_w(&mut self, rd: WReg, rn: WReg) {
        self.insn(enc::SXTH_W | (rn.bits() << 5) | rd.bits());
    }

    pub fn sxtb_w(&mut self, rd: WReg, rn: WReg) {
        self.insn(enc::SXTB_W | (rn.bits() << 5) | rd.bits());
    }

    pub fn cmp_w(&mut self, rn: WReg, rm: WReg) {
        self.insn(enc::CMP_W_REG | (rm.bits() << 16) | (rn.bits() << 5));
    }

    pub fn cmp_x(&mut self, rn: XReg, rm: XReg) {
        self.insn(enc::CMP_X_REG | (rm.bits() << 16) | (rn.bits() << 5));
    }

    pub fn cmp_w_imm(&mut self, rn: WReg, imm: u32) -> AsmResult {
        if imm > 4095 {
            return Err(AsmError::InvalidImmediate);
        }
        self.insn(enc::CMP_W_IMM | (imm << 10) | (rn.bits() << 5));
        Ok(())
    }

    pub fn cset_w(&mut self, rd: WReg, cond: Cond) {
        let inverted = cond.inverted() as u32;
        self.insn(enc::CSET_W | (31 << 16) | (inverted << 12) | (31 << 5) | rd.bits());
    }

    pub fn csel_w(&mut self, rd: WReg, rn: WReg, rm: WReg, cond: Cond) {
        self.insn(
            enc::CSEL_W | (rm.bits() << 16) | ((cond as u32) << 12) | (rn.bits() << 5) | rd.bits(),
        );
    }

    pub fn fmov_s_from_w(&mut self, vd: VReg, rn: WReg) {
        self.insn(enc::FMOV_S_FROM_W | (rn.bits() << 5) | vd.bits());
    }

    pub fn fmov_w_from_s(&mut self, rd: WReg, vn: VReg) {
        self.insn(enc::FMOV_W_FROM_S | (vn.bits() << 5) | rd.bits());
    }

    pub fn fmov_d_from_x(&mut self, vd: VReg, rn: XReg) {
        self.insn(enc::FMOV_D_FROM_X | (rn.bits() << 5) | vd.bits());
    }

    pub fn fmov_x_from_d(&mut self, rd: XReg, vn: VReg) {
        self.insn(enc::FMOV_X_FROM_D | (vn.bits() << 5) | rd.bits());
    }

    pub fn fcmp_s(&mut self, vn: VReg, vm: VReg) {
        self.insn(enc::FCMP_S | (vm.bits() << 16) | (vn.bits() << 5));
    }

    pub fn fcmp_d(&mut self, vn: VReg, vm: VReg) {
        self.insn(enc::FCMP_D | (vm.bits() << 16) | (vn.bits() << 5));
    }

    pub fn fadd_d(&mut self, vd: VReg, vn: VReg, vm: VReg) {
        self.insn(enc::FADD_D | (vm.bits() << 16) | (vn.bits() << 5) | vd.bits());
    }

    pub fn fadd_s(&mut self, vd: VReg, vn: VReg, vm: VReg) {
        self.insn(enc::FADD_S | (vm.bits() << 16) | (vn.bits() << 5) | vd.bits());
    }

    pub fn fsub_d(&mut self, vd: VReg, vn: VReg, vm: VReg) {
        self.insn(enc::FSUB_D | (vm.bits() << 16) | (vn.bits() << 5) | vd.bits());
    }

    pub fn fsub_s(&mut self, vd: VReg, vn: VReg, vm: VReg) {
        self.insn(enc::FSUB_S | (vm.bits() << 16) | (vn.bits() << 5) | vd.bits());
    }

    pub fn fmul_d(&mut self, vd: VReg, vn: VReg, vm: VReg) {
        self.insn(enc::FMUL_D | (vm.bits() << 16) | (vn.bits() << 5) | vd.bits());
    }

    pub fn fmul_s(&mut self, vd: VReg, vn: VReg, vm: VReg) {
        self.insn(enc::FMUL_S | (vm.bits() << 16) | (vn.bits() << 5) | vd.bits());
    }

    pub fn fdiv_d(&mut self, vd: VReg, vn: VReg, vm: VReg) {
        self.insn(enc::FDIV_D | (vm.bits() << 16) | (vn.bits() << 5) | vd.bits());
    }

    pub fn fdiv_s(&mut self, vd: VReg, vn: VReg, vm: VReg) {
        self.insn(enc::FDIV_S | (vm.bits() << 16) | (vn.bits() << 5) | vd.bits());
    }

    pub fn fabs_s(&mut self, vd: VReg, vn: VReg) {
        self.insn(enc::FABS_S | (vn.bits() << 5) | vd.bits());
    }

    pub fn fabs_d(&mut self, vd: VReg, vn: VReg) {
        self.insn(enc::FABS_D | (vn.bits() << 5) | vd.bits());
    }

    pub fn fneg_s(&mut self, vd: VReg, vn: VReg) {
        self.insn(enc::FNEG_S | (vn.bits() << 5) | vd.bits());
    }

    pub fn fneg_d(&mut self, vd: VReg, vn: VReg) {
        self.insn(enc::FNEG_D | (vn.bits() << 5) | vd.bits());
    }

    pub fn fsqrt_s(&mut self, vd: VReg, vn: VReg) {
        self.insn(enc::FSQRT_S | (vn.bits() << 5) | vd.bits());
    }

    pub fn fsqrt_d(&mut self, vd: VReg, vn: VReg) {
        self.insn(enc::FSQRT_D | (vn.bits() << 5) | vd.bits());
    }

    pub fn frintn_s(&mut self, vd: VReg, vn: VReg) {
        self.insn(enc::FRINTN_S | (vn.bits() << 5) | vd.bits());
    }

    pub fn frintn_d(&mut self, vd: VReg, vn: VReg) {
        self.insn(enc::FRINTN_D | (vn.bits() << 5) | vd.bits());
    }

    pub fn frintp_s(&mut self, vd: VReg, vn: VReg) {
        self.insn(enc::FRINTP_S | (vn.bits() << 5) | vd.bits());
    }

    pub fn frintp_d(&mut self, vd: VReg, vn: VReg) {
        self.insn(enc::FRINTP_D | (vn.bits() << 5) | vd.bits());
    }

    pub fn frintm_s(&mut self, vd: VReg, vn: VReg) {
        self.insn(enc::FRINTM_S | (vn.bits() << 5) | vd.bits());
    }

    pub fn frintm_d(&mut self, vd: VReg, vn: VReg) {
        self.insn(enc::FRINTM_D | (vn.bits() << 5) | vd.bits());
    }

    pub fn frintz_s(&mut self, vd: VReg, vn: VReg) {
        self.insn(enc::FRINTZ_S | (vn.bits() << 5) | vd.bits());
    }

    pub fn frintz_d(&mut self, vd: VReg, vn: VReg) {
        self.insn(enc::FRINTZ_D | (vn.bits() << 5) | vd.bits());
    }

    pub fn cvtf_d_from_w(&mut self, vd: VReg, rn: WReg, signed: bool) {
        let base = if signed {
            enc::SCVTF_D_FROM_W
        } else {
            enc::UCVTF_D_FROM_W
        };
        self.insn(base | (rn.bits() << 5) | vd.bits());
    }

    pub fn cvtf_d_from_x(&mut self, vd: VReg, rn: XReg, signed: bool) {
        let base = if signed {
            enc::SCVTF_D_FROM_X
        } else {
            enc::UCVTF_D_FROM_X
        };
        self.insn(base | (rn.bits() << 5) | vd.bits());
    }

    pub fn fcvt_w_from_s(&mut self, rd: WReg, vn: VReg, signed: bool) {
        let base = if signed {
            enc::FCVTZS_W_FROM_S
        } else {
            enc::FCVTZU_W_FROM_S
        };
        self.insn(base | (vn.bits() << 5) | rd.bits());
    }

    pub fn fcvt_w_from_d(&mut self, rd: WReg, vn: VReg, signed: bool) {
        let base = if signed {
            enc::FCVTZS_W_FROM_D
        } else {
            enc::FCVTZU_W_FROM_D
        };
        self.insn(base | (vn.bits() << 5) | rd.bits());
    }

    pub fn ubfm_w(&mut self, rd: WReg, rn: WReg, immr: u32, imms: u32) {
        self.insn(enc::UBFM_W | (immr << 16) | (imms << 10) | (rn.bits() << 5) | rd.bits());
    }

    pub fn ubfm_x(&mut self, rd: XReg, rn: XReg, immr: u32, imms: u32) {
        self.insn(enc::UBFM_X | (immr << 16) | (imms << 10) | (rn.bits() << 5) | rd.bits());
    }

    pub fn lsl_x_imm(&mut self, rd: XReg, rn: XReg, shift: u32) -> AsmResult {
        if shift >= 64 {
            return Err(AsmError::InvalidImmediate);
        }
        self.ubfm_x(rd, rn, (64 - shift) & 63, 63 - shift);
        Ok(())
    }

    pub fn sbfm_w(&mut self, rd: WReg, rn: WReg, immr: u32, imms: u32) {
        self.insn(enc::SBFM_W | (immr << 16) | (imms << 10) | (rn.bits() << 5) | rd.bits());
    }

    pub fn sbfm_x(&mut self, rd: XReg, rn: XReg, immr: u32, imms: u32) {
        self.insn(enc::SBFM_X | (immr << 16) | (imms << 10) | (rn.bits() << 5) | rd.bits());
    }

    pub fn ldr_x_imm(&mut self, rt: XReg, rn: XReg, offset: usize) -> AsmResult {
        if offset % 8 != 0 || offset / 8 > 4095 {
            return Err(AsmError::InvalidImmediate);
        }
        self.insn(
            enc::LDR_X_UNSIGNED_IMM | (((offset / 8) as u32) << 10) | (rn.bits() << 5) | rt.bits(),
        );
        Ok(())
    }

    pub fn ldr_w_imm(&mut self, rt: WReg, rn: XReg, offset: usize) -> AsmResult {
        if offset % 4 != 0 || offset / 4 > 4095 {
            return Err(AsmError::InvalidImmediate);
        }
        self.insn(
            enc::LDR_W_UNSIGNED_IMM | (((offset / 4) as u32) << 10) | (rn.bits() << 5) | rt.bits(),
        );
        Ok(())
    }

    pub fn ldr_w_unscaled_imm(&mut self, rt: WReg, rn: XReg, offset: u32) -> AsmResult {
        if offset > 255 {
            return Err(AsmError::InvalidImmediate);
        }
        self.insn(enc::LDR_W_UNSCALED_IMM | (offset << 12) | (rn.bits() << 5) | rt.bits());
        Ok(())
    }

    pub fn ldrb_w(&mut self, rt: WReg, rn: XReg) {
        self.insn(enc::LDRB_W_UNSIGNED | (rn.bits() << 5) | rt.bits());
    }

    pub fn ldrsb_w(&mut self, rt: WReg, rn: XReg) {
        self.insn(enc::LDRSB_W_UNSIGNED | (rn.bits() << 5) | rt.bits());
    }

    pub fn ldrh_w(&mut self, rt: WReg, rn: XReg) {
        self.insn(enc::LDRH_W_UNSIGNED | (rn.bits() << 5) | rt.bits());
    }

    pub fn ldrsh_w(&mut self, rt: WReg, rn: XReg) {
        self.insn(enc::LDRSH_W_UNSIGNED | (rn.bits() << 5) | rt.bits());
    }

    pub fn ldr_w(&mut self, rt: WReg, rn: XReg) {
        self.insn(enc::LDR_W_UNSIGNED_IMM | (rn.bits() << 5) | rt.bits());
    }

    pub fn strb_w(&mut self, rt: WReg, rn: XReg) {
        self.insn(enc::STRB_W_UNSIGNED | (rn.bits() << 5) | rt.bits());
    }

    pub fn strh_w(&mut self, rt: WReg, rn: XReg) {
        self.insn(enc::STRH_W_UNSIGNED | (rn.bits() << 5) | rt.bits());
    }

    pub fn str_w(&mut self, rt: WReg, rn: XReg) {
        self.insn(enc::STR_W_UNSIGNED | (rn.bits() << 5) | rt.bits());
    }

    pub fn str_x_imm(&mut self, rt: XReg, rn: XReg, offset: usize) -> AsmResult {
        if offset % 8 != 0 || offset / 8 > 4095 {
            return Err(AsmError::InvalidImmediate);
        }
        self.insn(
            enc::STR_X_UNSIGNED_IMM | (((offset / 8) as u32) << 10) | (rn.bits() << 5) | rt.bits(),
        );
        Ok(())
    }

    pub fn str_w_imm(&mut self, rt: WReg, rn: XReg, offset: usize) -> AsmResult {
        if offset % 4 != 0 || offset / 4 > 4095 {
            return Err(AsmError::InvalidImmediate);
        }
        self.insn(
            enc::STR_W_UNSIGNED | (((offset / 4) as u32) << 10) | (rn.bits() << 5) | rt.bits(),
        );
        Ok(())
    }

    pub fn str_w_unscaled_imm(&mut self, rt: WReg, rn: XReg, offset: u32) -> AsmResult {
        if offset > 255 {
            return Err(AsmError::InvalidImmediate);
        }
        self.insn(enc::STR_W_UNSCALED_IMM | (offset << 12) | (rn.bits() << 5) | rt.bits());
        Ok(())
    }

    fn emit_r3(&mut self, base: u32, rd: u32, rn: u32, rm: u32) {
        self.insn(base | (rm << 16) | (rn << 5) | rd);
    }
}

#[derive(Debug, Clone)]
pub struct A64BaselineMasm {
    inner: A64Masm,
}

impl A64BaselineMasm {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: A64Masm::with_capacity(capacity),
        }
    }

    pub fn offset(&self) -> usize {
        self.inner.offset()
    }

    pub fn truncate(&mut self, len: usize) {
        self.inner.truncate(len);
    }

    pub fn as_mut_bytes(&mut self) -> &mut [u8] {
        self.inner.as_mut_bytes()
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.inner.into_bytes()
    }

    pub fn branch_placeholder(&mut self, kind: BranchKind) -> AsmResult<usize> {
        self.inner.branch_placeholder(kind)
    }

    pub fn ret(&mut self) {
        self.inner.ret();
    }

    pub fn mov_x_from_sp(&mut self, rd: u8) {
        self.inner.mov_x_from_sp(Self::x(rd));
    }

    pub fn blr_x(&mut self, rn: u8) {
        self.inner.blr_x(Self::x(rn));
    }

    pub fn mov_x(&mut self, rd: u8, rn: u8) {
        self.inner.mov_x(Self::x(rd), Self::x(rn));
    }

    pub fn mov_w(&mut self, rd: u8, rn: u8) {
        self.inner.mov_w(Self::w(rd), Self::w(rn));
    }

    pub fn mov_imm_u32(&mut self, rd: u8, value: u32) {
        self.inner.mov_imm_u32(Self::w(rd), value);
    }

    pub fn mov_imm_u64(&mut self, rd: u8, value: u64) {
        self.inner.mov_imm_u64(Self::x(rd), value);
    }

    pub fn stp_pre_x_sp(&mut self, rt: u8, rt2: u8) {
        self.inner.stp_pre_x_sp(Self::x(rt), Self::x(rt2));
    }

    pub fn stp_pre(&mut self, rt: u8, rt2: u8) {
        self.stp_pre_x_sp(rt, rt2);
    }

    pub fn ldp_post_x_sp(&mut self, rt: u8, rt2: u8) {
        self.inner.ldp_post_x_sp(Self::x(rt), Self::x(rt2));
    }

    pub fn ldp_post(&mut self, rt: u8, rt2: u8) {
        self.ldp_post_x_sp(rt, rt2);
    }

    pub fn add_imm_u32(&mut self, rd: u8, rn: u8, imm: u32) -> AsmResult {
        if imm <= 4095 {
            self.inner.add_w_imm(Self::w(rd), Self::w(rn), imm)?;
        } else {
            self.mov_imm_u32(17, imm);
            self.add_w(rd, rn, 17);
        }
        Ok(())
    }

    pub fn add_imm_u64(&mut self, rd: u8, rn: u8, imm: u64) -> AsmResult {
        if imm <= 4095 {
            self.inner.add_x_imm(Self::x(rd), Self::x(rn), imm as u32)?;
        } else {
            self.mov_imm_u64(17, imm);
            self.add_x(rd, rn, 17);
        }
        Ok(())
    }

    pub fn sub_imm_u32(&mut self, rd: u8, rn: u8, imm: u32) -> AsmResult {
        self.inner.sub_w_imm(Self::w(rd), Self::w(rn), imm)
    }

    pub fn add_w(&mut self, rd: u8, rn: u8, rm: u8) {
        self.inner.add_w(Self::w(rd), Self::w(rn), Self::w(rm));
    }

    pub fn add_x(&mut self, rd: u8, rn: u8, rm: u8) {
        self.inner.add_x(Self::x(rd), Self::x(rn), Self::x(rm));
    }

    pub fn sub_w(&mut self, rd: u8, rn: u8, rm: u8) {
        self.inner.sub_w(Self::w(rd), Self::w(rn), Self::w(rm));
    }

    pub fn sub_x(&mut self, rd: u8, rn: u8, rm: u8) {
        self.inner.sub_x(Self::x(rd), Self::x(rn), Self::x(rm));
    }

    pub fn mul_w(&mut self, rd: u8, rn: u8, rm: u8) {
        self.inner.mul_w(Self::w(rd), Self::w(rn), Self::w(rm));
    }

    pub fn mul_x(&mut self, rd: u8, rn: u8, rm: u8) {
        self.inner.mul_x(Self::x(rd), Self::x(rn), Self::x(rm));
    }

    pub fn udiv_w(&mut self, rd: u8, rn: u8, rm: u8) {
        self.inner.udiv_w(Self::w(rd), Self::w(rn), Self::w(rm));
    }

    pub fn sdiv_w(&mut self, rd: u8, rn: u8, rm: u8) {
        self.inner.sdiv_w(Self::w(rd), Self::w(rn), Self::w(rm));
    }

    pub fn udiv_x(&mut self, rd: u8, rn: u8, rm: u8) {
        self.inner.udiv_x(Self::x(rd), Self::x(rn), Self::x(rm));
    }

    pub fn sdiv_x(&mut self, rd: u8, rn: u8, rm: u8) {
        self.inner.sdiv_x(Self::x(rd), Self::x(rn), Self::x(rm));
    }

    pub fn msub_w(&mut self, rd: u8, rn: u8, rm: u8, ra: u8) {
        self.inner
            .msub_w(Self::w(rd), Self::w(rn), Self::w(rm), Self::w(ra));
    }

    pub fn msub_x(&mut self, rd: u8, rn: u8, rm: u8, ra: u8) {
        self.inner
            .msub_x(Self::x(rd), Self::x(rn), Self::x(rm), Self::x(ra));
    }

    pub fn neg_w(&mut self, rd: u8, rm: u8) {
        self.inner.neg_w(Self::w(rd), Self::w(rm));
    }

    pub fn neg_x(&mut self, rd: u8, rm: u8) {
        self.inner.neg_x(Self::x(rd), Self::x(rm));
    }

    pub fn and_w(&mut self, rd: u8, rn: u8, rm: u8) {
        self.inner.and_w(Self::w(rd), Self::w(rn), Self::w(rm));
    }

    pub fn and_x(&mut self, rd: u8, rn: u8, rm: u8) {
        self.inner.and_x(Self::x(rd), Self::x(rn), Self::x(rm));
    }

    pub fn orr_w(&mut self, rd: u8, rn: u8, rm: u8) {
        self.inner.orr_w(Self::w(rd), Self::w(rn), Self::w(rm));
    }

    pub fn orr_x(&mut self, rd: u8, rn: u8, rm: u8) {
        self.inner.orr_x(Self::x(rd), Self::x(rn), Self::x(rm));
    }

    pub fn eor_w(&mut self, rd: u8, rn: u8, rm: u8) {
        self.inner.eor_w(Self::w(rd), Self::w(rn), Self::w(rm));
    }

    pub fn eor_x(&mut self, rd: u8, rn: u8, rm: u8) {
        self.inner.eor_x(Self::x(rd), Self::x(rn), Self::x(rm));
    }

    pub fn lslv_w(&mut self, rd: u8, rn: u8, rm: u8) {
        self.inner.lslv_w(Self::w(rd), Self::w(rn), Self::w(rm));
    }

    pub fn lslv_x(&mut self, rd: u8, rn: u8, rm: u8) {
        self.inner.lslv_x(Self::x(rd), Self::x(rn), Self::x(rm));
    }

    pub fn lsrv_w(&mut self, rd: u8, rn: u8, rm: u8) {
        self.inner.lsrv_w(Self::w(rd), Self::w(rn), Self::w(rm));
    }

    pub fn lsrv_x(&mut self, rd: u8, rn: u8, rm: u8) {
        self.inner.lsrv_x(Self::x(rd), Self::x(rn), Self::x(rm));
    }

    pub fn asrv_w(&mut self, rd: u8, rn: u8, rm: u8) {
        self.inner.asrv_w(Self::w(rd), Self::w(rn), Self::w(rm));
    }

    pub fn asrv_x(&mut self, rd: u8, rn: u8, rm: u8) {
        self.inner.asrv_x(Self::x(rd), Self::x(rn), Self::x(rm));
    }

    pub fn rorv_w(&mut self, rd: u8, rn: u8, rm: u8) {
        self.inner.rorv_w(Self::w(rd), Self::w(rn), Self::w(rm));
    }

    pub fn rorv_x(&mut self, rd: u8, rn: u8, rm: u8) {
        self.inner.rorv_x(Self::x(rd), Self::x(rn), Self::x(rm));
    }

    pub fn rbit_w(&mut self, rd: u8, rn: u8) {
        self.inner.rbit_w(Self::w(rd), Self::w(rn));
    }

    pub fn rbit_x(&mut self, rd: u8, rn: u8) {
        self.inner.rbit_x(Self::x(rd), Self::x(rn));
    }

    pub fn clz_w(&mut self, rd: u8, rn: u8) {
        self.inner.clz_w(Self::w(rd), Self::w(rn));
    }

    pub fn clz_x(&mut self, rd: u8, rn: u8) {
        self.inner.clz_x(Self::x(rd), Self::x(rn));
    }

    pub fn sxth_w(&mut self, rd: u8, rn: u8) {
        self.inner.sxth_w(Self::w(rd), Self::w(rn));
    }

    pub fn sxtb_w(&mut self, rd: u8, rn: u8) {
        self.inner.sxtb_w(Self::w(rd), Self::w(rn));
    }

    pub fn cmp_w(&mut self, rn: u8, rm: u8) {
        self.inner.cmp_w(Self::w(rn), Self::w(rm));
    }

    pub fn cmp_x(&mut self, rn: u8, rm: u8) {
        self.inner.cmp_x(Self::x(rn), Self::x(rm));
    }

    pub fn cmp_w_imm(&mut self, rn: u8, imm: u32) -> AsmResult {
        self.inner.cmp_w_imm(Self::w(rn), imm)
    }

    pub fn cmp_w_u32(&mut self, rn: u8, imm: u32) {
        if imm <= 4095 {
            self.cmp_w_imm(rn, imm).expect("checked compare immediate");
        } else {
            self.mov_imm_u32(17, imm);
            self.cmp_w(rn, 17);
        }
    }

    pub fn cset_w(&mut self, rd: u8, cond: Cond) {
        self.inner.cset_w(Self::w(rd), cond);
    }

    pub fn csel_w(&mut self, rd: u8, rn: u8, rm: u8, cond: Cond) {
        self.inner
            .csel_w(Self::w(rd), Self::w(rn), Self::w(rm), cond);
    }

    pub fn fmov_s_from_w(&mut self, vd: u8, rn: u8) {
        self.inner.fmov_s_from_w(Self::v(vd), Self::w(rn));
    }

    pub fn fmov_w_from_s(&mut self, rd: u8, vn: u8) {
        self.inner.fmov_w_from_s(Self::w(rd), Self::v(vn));
    }

    pub fn fmov_d_from_x(&mut self, vd: u8, rn: u8) {
        self.inner.fmov_d_from_x(Self::v(vd), Self::x(rn));
    }

    pub fn fmov_x_from_d(&mut self, rd: u8, vn: u8) {
        self.inner.fmov_x_from_d(Self::x(rd), Self::v(vn));
    }

    pub fn fcmp_s(&mut self, vn: u8, vm: u8) {
        self.inner.fcmp_s(Self::v(vn), Self::v(vm));
    }

    pub fn fcmp_d(&mut self, vn: u8, vm: u8) {
        self.inner.fcmp_d(Self::v(vn), Self::v(vm));
    }

    pub fn fadd_d(&mut self, vd: u8, vn: u8, vm: u8) {
        self.inner.fadd_d(Self::v(vd), Self::v(vn), Self::v(vm));
    }

    pub fn fadd_s(&mut self, vd: u8, vn: u8, vm: u8) {
        self.inner.fadd_s(Self::v(vd), Self::v(vn), Self::v(vm));
    }

    pub fn fsub_d(&mut self, vd: u8, vn: u8, vm: u8) {
        self.inner.fsub_d(Self::v(vd), Self::v(vn), Self::v(vm));
    }

    pub fn fsub_s(&mut self, vd: u8, vn: u8, vm: u8) {
        self.inner.fsub_s(Self::v(vd), Self::v(vn), Self::v(vm));
    }

    pub fn fmul_d(&mut self, vd: u8, vn: u8, vm: u8) {
        self.inner.fmul_d(Self::v(vd), Self::v(vn), Self::v(vm));
    }

    pub fn fmul_s(&mut self, vd: u8, vn: u8, vm: u8) {
        self.inner.fmul_s(Self::v(vd), Self::v(vn), Self::v(vm));
    }

    pub fn fdiv_d(&mut self, vd: u8, vn: u8, vm: u8) {
        self.inner.fdiv_d(Self::v(vd), Self::v(vn), Self::v(vm));
    }

    pub fn fdiv_s(&mut self, vd: u8, vn: u8, vm: u8) {
        self.inner.fdiv_s(Self::v(vd), Self::v(vn), Self::v(vm));
    }

    pub fn fabs_s(&mut self, vd: u8, vn: u8) {
        self.inner.fabs_s(Self::v(vd), Self::v(vn));
    }

    pub fn fabs_d(&mut self, vd: u8, vn: u8) {
        self.inner.fabs_d(Self::v(vd), Self::v(vn));
    }

    pub fn fneg_s(&mut self, vd: u8, vn: u8) {
        self.inner.fneg_s(Self::v(vd), Self::v(vn));
    }

    pub fn fneg_d(&mut self, vd: u8, vn: u8) {
        self.inner.fneg_d(Self::v(vd), Self::v(vn));
    }

    pub fn fsqrt_s(&mut self, vd: u8, vn: u8) {
        self.inner.fsqrt_s(Self::v(vd), Self::v(vn));
    }

    pub fn fsqrt_d(&mut self, vd: u8, vn: u8) {
        self.inner.fsqrt_d(Self::v(vd), Self::v(vn));
    }

    pub fn frintn_s(&mut self, vd: u8, vn: u8) {
        self.inner.frintn_s(Self::v(vd), Self::v(vn));
    }

    pub fn frintn_d(&mut self, vd: u8, vn: u8) {
        self.inner.frintn_d(Self::v(vd), Self::v(vn));
    }

    pub fn frintp_s(&mut self, vd: u8, vn: u8) {
        self.inner.frintp_s(Self::v(vd), Self::v(vn));
    }

    pub fn frintp_d(&mut self, vd: u8, vn: u8) {
        self.inner.frintp_d(Self::v(vd), Self::v(vn));
    }

    pub fn frintm_s(&mut self, vd: u8, vn: u8) {
        self.inner.frintm_s(Self::v(vd), Self::v(vn));
    }

    pub fn frintm_d(&mut self, vd: u8, vn: u8) {
        self.inner.frintm_d(Self::v(vd), Self::v(vn));
    }

    pub fn frintz_s(&mut self, vd: u8, vn: u8) {
        self.inner.frintz_s(Self::v(vd), Self::v(vn));
    }

    pub fn frintz_d(&mut self, vd: u8, vn: u8) {
        self.inner.frintz_d(Self::v(vd), Self::v(vn));
    }

    pub fn cvtf_d_from_w(&mut self, vd: u8, rn: u8, signed: bool) {
        self.inner.cvtf_d_from_w(Self::v(vd), Self::w(rn), signed);
    }

    pub fn cvtf_d_from_x(&mut self, vd: u8, rn: u8, signed: bool) {
        self.inner.cvtf_d_from_x(Self::v(vd), Self::x(rn), signed);
    }

    pub fn fcvt_w_from_s(&mut self, rd: u8, vn: u8, signed: bool) {
        self.inner.fcvt_w_from_s(Self::w(rd), Self::v(vn), signed);
    }

    pub fn fcvt_w_from_d(&mut self, rd: u8, vn: u8, signed: bool) {
        self.inner.fcvt_w_from_d(Self::w(rd), Self::v(vn), signed);
    }

    pub fn lsl_w_imm(&mut self, rd: u8, rn: u8, shift: u32) {
        self.ubfm_w(rd, rn, (32 - shift) & 31, 31 - shift);
    }

    pub fn lsr_w_imm(&mut self, rd: u8, rn: u8, shift: u32) {
        self.ubfm_w(rd, rn, shift, 31);
    }

    pub fn asr_w_imm(&mut self, rd: u8, rn: u8, shift: u32) {
        self.sbfm_w(rd, rn, shift, 31);
    }

    pub fn ubfm_w(&mut self, rd: u8, rn: u8, immr: u32, imms: u32) {
        self.inner.ubfm_w(Self::w(rd), Self::w(rn), immr, imms);
    }

    pub fn ubfm_x(&mut self, rd: u8, rn: u8, immr: u32, imms: u32) {
        self.inner.ubfm_x(Self::x(rd), Self::x(rn), immr, imms);
    }

    pub fn sbfm_w(&mut self, rd: u8, rn: u8, immr: u32, imms: u32) {
        self.inner.sbfm_w(Self::w(rd), Self::w(rn), immr, imms);
    }

    pub fn sbfm_x(&mut self, rd: u8, rn: u8, immr: u32, imms: u32) {
        self.inner.sbfm_x(Self::x(rd), Self::x(rn), immr, imms);
    }

    pub fn lsl_x_imm(&mut self, rd: u8, rn: u8, shift: u32) -> AsmResult {
        self.inner.lsl_x_imm(Self::x(rd), Self::x(rn), shift)
    }

    pub fn lsr_x_imm(&mut self, rd: u8, rn: u8, shift: u32) -> AsmResult {
        if shift >= 64 {
            return Err(AsmError::InvalidImmediate);
        }
        self.ubfm_x(rd, rn, shift, 63);
        Ok(())
    }

    pub fn ldr_x_imm(&mut self, rt: u8, rn: u8, offset: usize) -> AsmResult {
        self.inner.ldr_x_imm(Self::x(rt), Self::x(rn), offset)
    }

    pub fn ldr_w_imm(&mut self, rt: u8, rn: u8, offset: usize) -> AsmResult {
        self.inner.ldr_w_imm(Self::w(rt), Self::x(rn), offset)
    }

    pub fn ldr_w_unscaled_imm(&mut self, rt: u8, rn: u8, offset: u32) -> AsmResult {
        self.inner
            .ldr_w_unscaled_imm(Self::w(rt), Self::x(rn), offset)
    }

    pub fn ldrb_w(&mut self, rt: u8, rn: u8) {
        self.inner.ldrb_w(Self::w(rt), Self::x(rn));
    }

    pub fn ldrsb_w(&mut self, rt: u8, rn: u8) {
        self.inner.ldrsb_w(Self::w(rt), Self::x(rn));
    }

    pub fn ldrh_w(&mut self, rt: u8, rn: u8) {
        self.inner.ldrh_w(Self::w(rt), Self::x(rn));
    }

    pub fn ldrsh_w(&mut self, rt: u8, rn: u8) {
        self.inner.ldrsh_w(Self::w(rt), Self::x(rn));
    }

    pub fn ldr_w(&mut self, rt: u8, rn: u8) {
        self.inner.ldr_w(Self::w(rt), Self::x(rn));
    }

    pub fn strb_w(&mut self, rt: u8, rn: u8) {
        self.inner.strb_w(Self::w(rt), Self::x(rn));
    }

    pub fn strh_w(&mut self, rt: u8, rn: u8) {
        self.inner.strh_w(Self::w(rt), Self::x(rn));
    }

    pub fn str_w(&mut self, rt: u8, rn: u8) {
        self.inner.str_w(Self::w(rt), Self::x(rn));
    }

    pub fn str_x_imm(&mut self, rt: u8, rn: u8, offset: usize) -> AsmResult {
        self.inner.str_x_imm(Self::x(rt), Self::x(rn), offset)
    }

    pub fn str_w_unscaled_imm(&mut self, rt: u8, rn: u8, offset: u32) -> AsmResult {
        self.inner
            .str_w_unscaled_imm(Self::w(rt), Self::x(rn), offset)
    }

    fn x(reg: u8) -> XReg {
        XReg::new_unchecked(reg)
    }

    fn w(reg: u8) -> WReg {
        WReg::new_unchecked(reg)
    }

    fn v(reg: u8) -> VReg {
        VReg::new_unchecked(reg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_register_numbers() {
        assert!(XReg::new(31).is_ok());
        assert_eq!(XReg::new(32), Err(AsmError::InvalidRegister));
    }

    #[test]
    fn emits_mul_x_encoding() {
        let mut asm = A64Masm::with_capacity(4);
        asm.mul_x(
            XReg::new_unchecked(1),
            XReg::new_unchecked(2),
            XReg::new_unchecked(3),
        );
        let expected = enc::MUL_X | (3 << 16) | (2 << 5) | 1;
        assert_eq!(asm.into_bytes(), expected.to_le_bytes());
    }

    #[test]
    fn emits_ret_encoding() {
        let mut asm = A64Masm::with_capacity(4);
        asm.ret();
        assert_eq!(asm.into_bytes(), enc::RET.to_le_bytes());
    }

    #[test]
    fn patches_cond_branch_forward() {
        let mut bytes = (enc::B_COND | Cond::Eq as u32).to_le_bytes();
        patch_branch(&mut bytes, 0, 8, BranchKind::BCond(Cond::Eq)).expect("patch");
        let expected = enc::B_COND | (2 << 5) | Cond::Eq as u32;
        assert_eq!(u32::from_le_bytes(bytes), expected);
    }
}
