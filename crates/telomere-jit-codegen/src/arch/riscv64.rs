use crate::{
    masm::{AsmError, AsmResult, PhysLoc},
    target::{TargetArch, TargetInfo, TargetOs},
};

pub use crate::masm::{BranchKind, Cond};

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
const S1: u8 = 9;
const S2: u8 = 18;
const S3: u8 = 19;
const S4: u8 = 20;
const S5: u8 = 21;
const S6: u8 = 22;
const S7: u8 = 23;
const S8: u8 = 24;
const S9: u8 = 25;
const S10: u8 = 26;
const S11: u8 = 27;
const T3: u8 = 28;
const T4: u8 = 29;
const T5: u8 = 30;
const T6: u8 = 31;

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

/// Patches the eight-byte branch placeholder at `at` (the
/// `auipc`/`jalr` pair emitted by
/// [`Riscv64BaselineMasm::branch_placeholder`]) to jump to `target`.
///
/// The pair reaches ±2 GiB from the placeholder. A lone `jal` spans
/// only ±1 MiB, and the fixups that target a function's epilogue sit
/// near its start, so any function whose emitted code grows past
/// that range was rejected outright at patch time (#310).
pub fn patch_branch(bytes: &mut [u8], at: usize, target: usize, _kind: BranchKind) -> AsmResult {
    let delta = target as isize - at as isize;
    let (hi, lo) = branch_pair_offsets(delta).ok_or(AsmError::BranchOutOfRange)?;
    let jalr = encode_i(0x67, 0, ZERO, T0, lo).ok_or(AsmError::InvalidImmediate)?;
    let site = bytes
        .get_mut(at..at + 8)
        .ok_or(AsmError::InvalidImmediate)?;
    site[..4].copy_from_slice(&encode_auipc(T0, hi).to_le_bytes());
    site[4..].copy_from_slice(&jalr.to_le_bytes());
    Ok(())
}

/// Splits a PC-relative byte offset into the `imm20`/`imm12` pair an
/// `auipc` + `jalr` sequence applies: `auipc` adds `imm20 << 12` to
/// its own PC and `jalr` then adds the sign-extended `imm12`.
/// Together they cover ±2 GiB; returns `None` outside that range.
fn branch_pair_offsets(delta: isize) -> Option<(i32, i32)> {
    let hi = (delta + 0x800) >> 12;
    if !(-(1 << 19)..(1 << 19)).contains(&hi) {
        return None;
    }
    let lo = delta - (hi << 12);
    Some((hi as i32, lo as i32))
}

fn encode_auipc(rd: u8, imm20: i32) -> u32 {
    (((imm20 as u32) & 0x000f_ffff) << 12) | ((rd as u32) << 7) | 0x17
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
            BranchKind::B => Ok(self.branch_pair_placeholder()),
            BranchKind::BCond(cond) => {
                let cmp = self.last_cmp.ok_or(AsmError::InvalidImmediate)?;
                self.emit_cmp_branch_to_skip(cmp, cond.inverted())?;
                Ok(self.branch_pair_placeholder())
            }
            BranchKind::CbnzX(reg) => {
                self.load_vreg64(T0, reg)?;
                self.insn(encode_b(0b000, T0, ZERO, 12).expect("local branch fits"));
                Ok(self.branch_pair_placeholder())
            }
            BranchKind::CbnzW(reg) => {
                self.load_vreg32(T0, reg)?;
                self.insn(encode_b(0b000, T0, ZERO, 12).expect("local branch fits"));
                Ok(self.branch_pair_placeholder())
            }
            // CbzW is an aarch64-only placeholder shape.
            BranchKind::CbzW(_) => Err(AsmError::UnsupportedFeature),
        }
    }

    /// Emits the eight-byte long-branch placeholder — an
    /// `auipc`/`jalr` pair through the T0 scratch — and returns its
    /// offset, the site [`patch_branch`] later rewrites. The pair is
    /// emitted with a zero offset, i.e. as a jump to itself. T0 is
    /// the assembler's universal scratch and is never a virtual
    /// register's home (#193), so clobbering it here is safe; the
    /// conditional placeholders have already consumed the value they
    /// test before this pair runs.
    fn branch_pair_placeholder(&mut self) -> usize {
        let at = self.offset();
        self.insn(encode_auipc(T0, 0));
        self.insn(encode_i(0x67, 0, ZERO, T0, 0).expect("jalr encodes"));
        at
    }

    pub fn ret(&mut self) {
        self.insn(encode_i(0x67, 0, ZERO, RA, 0).expect("ret encodes"));
    }

    pub fn mov_x_from_sp(&mut self, rd: u8) {
        let _ = rd;
    }

    pub fn blr_x(&mut self, rn: u8) {
        self.load_vreg64(T0, rn).expect("virtual register fits");
        // Argument vregs 0-7 are homed in A0-A7 (#193), so the
        // marshalling loads are identity moves; the generalized load
        // keeps the sequence correct for any future remapping.
        for (slot, reg) in [A0, A1, A2, A3, A4, A5, A6, A7].into_iter().enumerate() {
            self.load_vreg64(reg, slot as u8)
                .expect("argument slot fits");
        }
        self.insn(encode_i(0x67, 0, RA, T0, 0).expect("jalr encodes"));
        self.store_vreg64(0, A0).expect("return slot fits");
        self.store_vreg64(1, A1).expect("return slot fits");
    }

    pub fn mov_x(&mut self, rd: u8, rn: u8) {
        self.mov_vreg(rd, rn, Width::X64);
    }

    pub fn mov_w(&mut self, rd: u8, rn: u8) {
        self.mov_vreg(rd, rn, Width::W32);
    }

    pub fn mov_imm_u32(&mut self, rd: u8, value: u32) {
        self.li(T0, i64::from(value as i32));
        self.store_vreg32(rd, T0).expect("virtual register fits");
    }

    pub fn mov_imm_u64(&mut self, rd: u8, value: u64) {
        self.li(T0, value as i64);
        self.store_vreg64(rd, T0).expect("virtual register fits");
    }

    /// Relocation-site variant of the plain mov_imm (#197): the li
    /// part is fixed-length so the sequence can be patched in place.
    pub fn mov_imm_u32_reloc(&mut self, rd: u8, value: u32) {
        self.li_reloc32(T0, i64::from(value as i32));
        self.store_vreg32(rd, T0).expect("virtual register fits");
    }

    /// Relocation-site variant of the plain mov_imm (#197): the li
    /// part is fixed-length so the sequence can be patched in place.
    pub fn mov_imm_u64_reloc(&mut self, rd: u8, value: u64) {
        self.li_reloc(T0, value as i64);
        self.store_vreg64(rd, T0).expect("virtual register fits");
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
            // The virtual-register slots hang below S0 (slot_disp is
            // negative), so the slot base must sit below the saved
            // RA/S0 pair at the top of the frame. Anchoring S0 at
            // FRAME_SIZE put slot 0 on the RA spill (FRAME_SIZE - 8)
            // and slot 1 on the S0 spill (FRAME_SIZE - 16): the entry
            // argument spill then overwrote the saved return address,
            // and the epilogue returned to whatever value the body
            // last left in slot 0 (#161).
            self.addi(S0, SP, FRAME_SIZE - 16).expect("frame fits");
            // #193: the register-homed virtual registers keep their
            // values in physical registers for the whole body. The
            // argument registers A0-A7 are already the homes of
            // virtual registers 0-7, so no entry spill is needed for
            // them; the callee-saved homes (S1-S11, backing vregs
            // 19-28 and 11) are preserved into those vregs' own frame slots,
            // which the body never writes while the vreg is
            // register-homed, and the epilogue restores them.
            for (slot, reg) in [
                (11u8, S11),
                (19, S1),
                (20, S2),
                (21, S3),
                (22, S4),
                (23, S5),
                (24, S6),
                (25, S7),
                (26, S8),
                (27, S9),
                (28, S10),
            ] {
                self.store_slot64(slot, reg)
                    .expect("callee-saved home spill fits");
            }
        }
    }

    pub fn ldp_post_x_sp(&mut self, rt: u8, rt2: u8) {
        self.ldp_post(rt, rt2);
    }

    pub fn ldp_post(&mut self, rt: u8, rt2: u8) {
        if !self.epilogue_emitted && rt == 29 && rt2 == 30 {
            self.epilogue_emitted = true;
            // Virtual registers 0/1 are homed in A0/A1, so the return
            // values are already in place (#193).
            self.load_vreg64(A0, 0).expect("return slot fits");
            self.load_vreg64(A1, 1).expect("return slot fits");
            // Restore the caller's callee-saved registers from the
            // frame slots the prologue preserved them into.
            for (slot, reg) in [
                (11u8, S11),
                (19, S1),
                (20, S2),
                (21, S3),
                (22, S4),
                (23, S5),
                (24, S6),
                (25, S7),
                (26, S8),
                (27, S9),
                (28, S10),
            ] {
                self.load_slot64(reg, slot)
                    .expect("callee-saved home reload fits");
            }
            self.load_mem64(S0, SP, FRAME_SIZE - 16)
                .expect("s0 reload fits");
            self.load_mem64(RA, SP, FRAME_SIZE - 8)
                .expect("ra reload fits");
            self.addi(SP, SP, FRAME_SIZE).expect("frame fits");
        }
    }

    pub fn add_imm_u32(&mut self, rd: u8, rn: u8, imm: u32) -> AsmResult {
        self.load_vreg32(T0, rn)?;
        self.li(T1, i64::from(imm as i32));
        self.insn(encode_r(0x3b, 0, 0, T0, T0, T1));
        self.store_vreg32(rd, T0)
    }

    pub fn add_imm_u64(&mut self, rd: u8, rn: u8, imm: u64) -> AsmResult {
        self.load_vreg64(T0, rn)?;
        self.li(T1, imm as i64);
        self.insn(encode_r(0x33, 0, 0, T0, T0, T1));
        self.store_vreg64(rd, T0)
    }

    pub fn sub_imm_u32(&mut self, rd: u8, rn: u8, imm: u32) -> AsmResult {
        self.load_vreg32(T0, rn)?;
        self.li(T1, i64::from(imm as i32));
        self.insn(encode_r(0x3b, 0, 0x20, T0, T0, T1));
        self.store_vreg32(rd, T0)
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
        self.load_vreg32(T0, rn).expect("virtual register fits");
        self.load_vreg32(T1, rm).expect("virtual register fits");
        self.insn(encode_r(0x3b, 0, 0x01, T0, T0, T1));
        self.load_vreg32(T1, ra).expect("virtual register fits");
        self.insn(encode_r(0x3b, 0, 0x20, T0, T1, T0));
        self.store_vreg32(rd, T0).expect("virtual register fits");
    }

    pub fn msub_x(&mut self, rd: u8, rn: u8, rm: u8, ra: u8) {
        self.load_vreg64(T0, rn).expect("virtual register fits");
        self.load_vreg64(T1, rm).expect("virtual register fits");
        self.insn(encode_r(0x33, 0, 0x01, T0, T0, T1));
        self.load_vreg64(T1, ra).expect("virtual register fits");
        self.insn(encode_r(0x33, 0, 0x20, T0, T1, T0));
        self.store_vreg64(rd, T0).expect("virtual register fits");
    }

    pub fn neg_w(&mut self, rd: u8, rm: u8) {
        self.load_vreg32(T0, rm).expect("virtual register fits");
        self.insn(encode_r(0x3b, 0, 0x20, T0, ZERO, T0));
        self.store_vreg32(rd, T0).expect("virtual register fits");
    }

    pub fn neg_x(&mut self, rd: u8, rm: u8) {
        self.load_vreg64(T0, rm).expect("virtual register fits");
        self.insn(encode_r(0x33, 0, 0x20, T0, ZERO, T0));
        self.store_vreg64(rd, T0).expect("virtual register fits");
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
        self.load_vreg32(T0, rn).expect("virtual register fits");
        self.slli(T0, T0, 48);
        self.srai(T0, T0, 48);
        self.store_vreg32(rd, T0).expect("virtual register fits");
    }

    pub fn sxtb_w(&mut self, rd: u8, rn: u8) {
        self.load_vreg32(T0, rn).expect("virtual register fits");
        self.slli(T0, T0, 56);
        self.srai(T0, T0, 56);
        self.store_vreg32(rd, T0).expect("virtual register fits");
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
        let cmp = self.last_cmp.expect("cset follows cmp");
        self.emit_cmp_bool(T0, cmp, cond)
            .expect("comparison operands fit");
        self.store_vreg32(rd, T0).expect("virtual register fits");
    }

    pub fn csel_w(&mut self, rd: u8, rn: u8, rm: u8, cond: Cond) {
        // The deferred compare is not consumed by materialising it:
        // it describes the most recent compare until another compare
        // replaces it, mirroring hardware flags on the other backends.
        // cset and conditional branches read it the same way, so a
        // single compare may feed several conditional operations — a
        // multi-lane select emits one compare followed by one csel
        // per lane.
        let cmp = self.last_cmp.expect("csel follows cmp");
        self.emit_cmp_bool(T0, cmp, cond)
            .expect("comparison operands fit");
        self.load_vreg32(T1, rn).expect("virtual register fits");
        self.load_vreg32(T2, rm).expect("virtual register fits");
        self.insn(encode_b(0b001, T0, ZERO, 8).expect("local branch fits"));
        self.insn(encode_i(0x13, 0, T1, T2, 0).expect("mv encodes"));
        self.store_vreg32(rd, T1).expect("virtual register fits");
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
        self.load_vreg32(T0, vn).expect("virtual register fits");
        self.li(T1, 0x7fff_ffff);
        self.insn(encode_r(0x33, 7, 0, T0, T0, T1));
        self.store_vreg32(vd, T0).expect("virtual register fits");
    }

    pub fn fabs_d(&mut self, vd: u8, vn: u8) {
        self.load_vreg64(T0, vn).expect("virtual register fits");
        self.li(T1, 0x7fff_ffff_ffff_ffff);
        self.insn(encode_r(0x33, 7, 0, T0, T0, T1));
        self.store_vreg64(vd, T0).expect("virtual register fits");
    }

    pub fn fneg_s(&mut self, vd: u8, vn: u8) {
        self.load_vreg32(T0, vn).expect("virtual register fits");
        self.li(T1, 0x8000_0000u32 as i32 as i64);
        self.insn(encode_r(0x33, 4, 0, T0, T0, T1));
        self.store_vreg32(vd, T0).expect("virtual register fits");
    }

    pub fn fneg_d(&mut self, vd: u8, vn: u8) {
        self.load_vreg64(T0, vn).expect("virtual register fits");
        self.li(T1, 0x8000_0000_0000_0000u64 as i64);
        self.insn(encode_r(0x33, 4, 0, T0, T0, T1));
        self.store_vreg64(vd, T0).expect("virtual register fits");
    }

    pub fn fsqrt_s(&mut self, vd: u8, vn: u8) {
        self.float_unop(vd, vn, false, 0x2c, 0);
    }

    pub fn fsqrt_d(&mut self, vd: u8, vn: u8) {
        self.float_unop(vd, vn, true, 0x2d, 0);
    }

    pub fn frintn_s(&mut self, vd: u8, vn: u8) -> AsmResult {
        self.round(vd, vn, false, 0)
    }

    pub fn frintn_d(&mut self, vd: u8, vn: u8) -> AsmResult {
        self.round(vd, vn, true, 0)
    }

    pub fn frintp_s(&mut self, vd: u8, vn: u8) -> AsmResult {
        self.round(vd, vn, false, 3)
    }

    pub fn frintp_d(&mut self, vd: u8, vn: u8) -> AsmResult {
        self.round(vd, vn, true, 3)
    }

    pub fn frintm_s(&mut self, vd: u8, vn: u8) -> AsmResult {
        self.round(vd, vn, false, 2)
    }

    pub fn frintm_d(&mut self, vd: u8, vn: u8) -> AsmResult {
        self.round(vd, vn, true, 2)
    }

    pub fn frintz_s(&mut self, vd: u8, vn: u8) -> AsmResult {
        self.round(vd, vn, false, 1)
    }

    pub fn frintz_d(&mut self, vd: u8, vn: u8) -> AsmResult {
        self.round(vd, vn, true, 1)
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
        self.load_fvreg(0, vn, true).expect("virtual register fits");
        self.insn(encode_fp(0x20, 0, 0, 0, 1, 1, 0));
        self.store_fvreg(0, vd, false)
            .expect("virtual register fits");
    }

    pub fn fcvt_d_from_s(&mut self, vd: u8, vn: u8) {
        self.load_fvreg(0, vn, false)
            .expect("virtual register fits");
        self.insn(encode_fp(0x21, 0, 0, 0, 0, 0, 0));
        self.store_fvreg(0, vd, true)
            .expect("virtual register fits");
    }

    pub fn fcvt_w_from_s(&mut self, rd: u8, vn: u8, signed: bool) {
        self.fcvt_w(rd, vn, false, signed);
    }

    pub fn fcvt_w_from_d(&mut self, rd: u8, vn: u8, signed: bool) {
        self.fcvt_w(rd, vn, true, signed);
    }

    pub fn lsl_w_imm(&mut self, rd: u8, rn: u8, shift: u32) {
        self.load_vreg32(T0, rn).expect("virtual register fits");
        self.insn(encode_i(0x1b, 1, T0, T0, (shift & 31) as i32).expect("slliw encodes"));
        self.store_vreg32(rd, T0).expect("virtual register fits");
    }

    pub fn lsr_w_imm(&mut self, rd: u8, rn: u8, shift: u32) {
        self.load_vreg32(T0, rn).expect("virtual register fits");
        self.insn(encode_i(0x1b, 5, T0, T0, (shift & 31) as i32).expect("srliw encodes"));
        self.store_vreg32(rd, T0).expect("virtual register fits");
    }

    pub fn asr_w_imm(&mut self, rd: u8, rn: u8, shift: u32) {
        self.load_vreg32(T0, rn).expect("virtual register fits");
        self.insn(
            encode_i(0x1b, 5, T0, T0, ((0x20 << 5) | (shift & 31)) as i32).expect("sraiw encodes"),
        );
        self.store_vreg32(rd, T0).expect("virtual register fits");
    }

    pub fn ubfm_w(&mut self, rd: u8, rn: u8, immr: u32, imms: u32) {
        if immr == 0 {
            self.load_vreg32(T0, rn).expect("virtual register fits");
            let mask = if imms >= 31 {
                u32::MAX
            } else {
                (1u32 << (imms + 1)) - 1
            };
            self.li(T1, mask as i32 as i64);
            self.insn(encode_r(0x33, 7, 0, T0, T0, T1));
            self.store_vreg32(rd, T0).expect("virtual register fits");
        } else {
            self.lsr_w_imm(rd, rn, immr);
        }
    }

    pub fn ubfm_x(&mut self, rd: u8, rn: u8, immr: u32, imms: u32) {
        if immr == 0 {
            self.load_vreg64(T0, rn).expect("virtual register fits");
            if imms < 63 {
                self.li(T1, ((1u64 << (imms + 1)) - 1) as i64);
                self.insn(encode_r(0x33, 7, 0, T0, T0, T1));
            }
            self.store_vreg64(rd, T0).expect("virtual register fits");
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
        self.load_vreg64(T0, rn)?;
        self.slli(T0, T0, shift);
        self.store_vreg64(rd, T0)
    }

    pub fn lsr_x_imm(&mut self, rd: u8, rn: u8, shift: u32) -> AsmResult {
        if shift > 63 {
            return Err(AsmError::InvalidImmediate);
        }
        self.load_vreg64(T0, rn)?;
        self.srli(T0, T0, shift);
        self.store_vreg64(rd, T0)
    }

    pub fn ldr_x_imm(&mut self, rt: u8, rn: u8, offset: usize) -> AsmResult {
        self.load_vreg64(T0, rn)?;
        self.load_mem64(
            T0,
            T0,
            i32::try_from(offset).map_err(|_| AsmError::InvalidImmediate)?,
        )?;
        self.store_vreg64(rt, T0)
    }

    pub fn ldr_w_imm(&mut self, rt: u8, rn: u8, offset: usize) -> AsmResult {
        self.load_vreg64(T0, rn)?;
        self.load_mem32(
            T0,
            T0,
            i32::try_from(offset).map_err(|_| AsmError::InvalidImmediate)?,
        )?;
        self.store_vreg32(rt, T0)
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
        self.load_vreg64(T0, rn)?;
        self.load_vreg64(T1, rt)?;
        self.store_mem64(
            T0,
            i32::try_from(offset).map_err(|_| AsmError::InvalidImmediate)?,
            T1,
        )
    }

    pub fn str_w_imm(&mut self, rt: u8, rn: u8, offset: usize) -> AsmResult {
        self.load_vreg64(T0, rn)?;
        self.load_vreg32(T1, rt)?;
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

    /// The explicit per-target virtual-to-physical register mapping
    /// (#193). RISC-V has enough registers for a near 1:1 map:
    ///
    /// - vregs 0-7 → A0-A7: the entry arguments arrive in their homes
    ///   and helper-call marshalling is the identity;
    /// - vregs 19-28 → S1, S2-S10 (callee-saved): the call-preserved
    ///   set — ctx/code/locals and the operand-stack window — keeps
    ///   its values across helper calls with no spill traffic. The
    ///   prologue preserves the caller's S1-S10 into these vregs' own
    ///   frame slots and the epilogue restores them;
    /// - vreg 11 → S11 (callee-saved): one scratch vreg gets the same
    ///   preservation for free;
    /// - vregs 9/10 → T5/T6 (caller-saved): scratch vregs, dead
    ///   across calls by the same discipline aarch64 applies to its
    ///   caller-saved register file.
    ///
    /// Everything else stays in its frame slot. The encoder's own
    /// scratch registers T0-T4 and the frame base S0 are never homes.
    pub fn phys_loc(vreg: u8) -> AsmResult<PhysLoc> {
        Ok(match vreg {
            0..=7 => PhysLoc::Reg(A0 + vreg),
            9 => PhysLoc::Reg(T5),
            10 => PhysLoc::Reg(T6),
            11 => PhysLoc::Reg(S11),
            19 => PhysLoc::Reg(S1),
            20 => PhysLoc::Reg(S2),
            21 => PhysLoc::Reg(S3),
            22 => PhysLoc::Reg(S4),
            23 => PhysLoc::Reg(S5),
            24 => PhysLoc::Reg(S6),
            25 => PhysLoc::Reg(S7),
            26 => PhysLoc::Reg(S8),
            27 => PhysLoc::Reg(S9),
            28 => PhysLoc::Reg(S10),
            8 | 12..=18 | 29..=31 => PhysLoc::Slot(Self::slot_disp(vreg)?),
            _ => return Err(AsmError::InvalidRegister),
        })
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
        // Wrapping on purpose (#328): for values near i64::MAX the
        // sign-extended low chunk is negative, so the plain subtraction
        // overflows (a debug-build panic). The wrapped result is what
        // the emitted addi/slli sequence needs — the final addi wraps
        // in hardware and lands on the requested value.
        let high = value.wrapping_sub(i64::from(low)) >> 12;
        self.li(rd, high);
        self.slli(rd, rd, 12);
        if low != 0 {
            self.addi(rd, rd, low).expect("low li fits");
        }
    }

    /// Fixed-length variant of the plain mov_imm for relocation sites (#197):
    /// decomposes the value into six sign-extended 12-bit chunks with the
    /// same recurrence as li, but always emits every chunk (11
    /// instructions, 44 bytes), so a placeholder can be patched with any
    /// value without changing the sequence length.
    fn li_reloc(&mut self, rd: u8, value: i64) {
        let mut chunks = [0i32; 6];
        let mut rest = value;
        for chunk in &mut chunks {
            let low = ((rest << 52) >> 52) as i32;
            *chunk = low;
            rest = rest.wrapping_sub(i64::from(low)) >> 12;
        }
        debug_assert_eq!(rest, 0, "six 12-bit chunks cover any i64");
        self.addi(rd, ZERO, chunks[5]).expect("top li chunk fits");
        for chunk in chunks[..5].iter().rev() {
            self.slli(rd, rd, 12);
            self.addi(rd, rd, *chunk).expect("li chunk fits");
        }
    }

    /// Fixed-length  for a sign-extended 32-bit value (#197): three
    /// 12-bit chunks, always emitted (5 instructions, 20 bytes).
    fn li_reloc32(&mut self, rd: u8, value: i64) {
        debug_assert_eq!(
            value,
            i64::from(value as i32),
            "value must be a sign-extended i32"
        );
        let mut chunks = [0i32; 3];
        let mut rest = value;
        for chunk in &mut chunks {
            let low = ((rest << 52) >> 52) as i32;
            *chunk = low;
            rest = rest.wrapping_sub(i64::from(low)) >> 12;
        }
        debug_assert_eq!(rest, 0, "three 12-bit chunks cover a sign-extended i32");
        self.addi(rd, ZERO, chunks[2]).expect("top li chunk fits");
        for chunk in chunks[..2].iter().rev() {
            self.slli(rd, rd, 12);
            self.addi(rd, rd, *chunk).expect("li chunk fits");
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
        self.load_vreg64(T0, rn).expect("virtual register fits");
        self.srai(T0, T0, shift & 63);
        self.store_vreg64(rd, T0).expect("virtual register fits");
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

    /// Loads virtual register `vreg` into physical register `dst`
    /// (#193): a register move when `vreg` is register-homed (a no-op
    /// when it already lives in `dst`), a frame-slot load otherwise.
    fn load_vreg64(&mut self, dst: u8, vreg: u8) -> AsmResult {
        match Self::phys_loc(vreg)? {
            PhysLoc::Reg(reg) => {
                if reg != dst {
                    self.insn(encode_i(0x13, 0, dst, reg, 0).expect("mv encodes"));
                }
                Ok(())
            }
            PhysLoc::Slot(_) => self.load_slot64(dst, vreg),
        }
    }

    /// The 32-bit load: register-homed values are zero-extended into
    /// `dst`, matching the zero-extending slot load (lw with the
    /// register file's unsigned form), so a 32-bit read never leaks
    /// upper bits the slot form would have discarded.
    fn load_vreg32(&mut self, dst: u8, vreg: u8) -> AsmResult {
        match Self::phys_loc(vreg)? {
            PhysLoc::Reg(reg) => {
                self.slli(dst, reg, 32);
                self.srli(dst, dst, 32);
                Ok(())
            }
            PhysLoc::Slot(_) => self.load_slot32(dst, vreg),
        }
    }

    fn load_vreg_width(&mut self, dst: u8, vreg: u8, width: Width) -> AsmResult {
        match width {
            Width::W32 => self.load_vreg32(dst, vreg),
            Width::X64 => self.load_vreg64(dst, vreg),
        }
    }

    /// Stores physical register `src` into virtual register `vreg`
    /// (#193): a register move when `vreg` is register-homed, a
    /// frame-slot store otherwise.
    fn store_vreg64(&mut self, vreg: u8, src: u8) -> AsmResult {
        match Self::phys_loc(vreg)? {
            PhysLoc::Reg(reg) => {
                if reg != src {
                    self.insn(encode_i(0x13, 0, reg, src, 0).expect("mv encodes"));
                }
                Ok(())
            }
            PhysLoc::Slot(_) => self.store_slot64(vreg, src),
        }
    }

    /// The 32-bit store: a register-homed destination is written
    /// through a zero-extending move so the home register keeps the
    /// same canonical form the slot store produces.
    fn store_vreg32(&mut self, vreg: u8, src: u8) -> AsmResult {
        match Self::phys_loc(vreg)? {
            PhysLoc::Reg(reg) => {
                self.slli(reg, src, 32);
                self.srli(reg, reg, 32);
                Ok(())
            }
            PhysLoc::Slot(_) => self.store_slot32(vreg, src),
        }
    }

    fn mov_vreg(&mut self, rd: u8, rn: u8, width: Width) {
        match (Self::phys_loc(rd), Self::phys_loc(rn)) {
            (Ok(PhysLoc::Reg(dst)), Ok(PhysLoc::Reg(src))) => {
                // Register-to-register: 64-bit moves copy verbatim;
                // 32-bit moves zero-extend like the slot round-trip.
                match width {
                    Width::X64 => {
                        if dst != src {
                            self.insn(encode_i(0x13, 0, dst, src, 0).expect("mv encodes"));
                        }
                    }
                    Width::W32 => {
                        self.slli(dst, src, 32);
                        self.srli(dst, dst, 32);
                    }
                }
            }
            (Ok(PhysLoc::Reg(dst)), Ok(PhysLoc::Slot(_))) => match width {
                Width::X64 => self.load_slot64(dst, rn).expect("virtual register fits"),
                Width::W32 => self.load_slot32(dst, rn).expect("virtual register fits"),
            },
            (Ok(PhysLoc::Slot(_)), Ok(PhysLoc::Reg(src))) => match width {
                Width::X64 => self.store_slot64(rd, src).expect("virtual register fits"),
                Width::W32 => self.store_slot32(rd, src).expect("virtual register fits"),
            },
            (Ok(PhysLoc::Slot(_)), Ok(PhysLoc::Slot(_))) => match width {
                Width::X64 => {
                    self.load_slot64(T0, rn).expect("virtual register fits");
                    self.store_slot64(rd, T0).expect("virtual register fits");
                }
                Width::W32 => {
                    self.load_slot32(T0, rn).expect("virtual register fits");
                    self.store_slot32(rd, T0).expect("virtual register fits");
                }
            },
            _ => panic!("virtual register fits"),
        }
    }

    /// Loads virtual register `vreg` into FP register `fd` (#193):
    /// register-homed values move through fmv, slot-resident values
    /// keep the direct frame-slot FP load.
    fn load_fvreg(&mut self, fd: u8, vreg: u8, f64: bool) -> AsmResult {
        match Self::phys_loc(vreg)? {
            PhysLoc::Reg(reg) => {
                // fmv.d.x = funct7 0x79, fmv.s.x = funct7 0x78.
                let funct7 = if f64 { 0x79 } else { 0x78 };
                self.insn(encode_fp(funct7, 0, fd, reg, 0, 0, 0));
                Ok(())
            }
            PhysLoc::Slot(_) => self.load_fslot(fd, vreg, f64),
        }
    }

    /// Stores FP register `fs` into virtual register `vreg` (#193):
    /// register-homed destinations move through fmv into the home
    /// register, slot-resident ones keep the direct FP store.
    fn store_fvreg(&mut self, fs: u8, vreg: u8, f64: bool) -> AsmResult {
        match Self::phys_loc(vreg)? {
            PhysLoc::Reg(reg) => {
                // fmv.x.d = funct7 0x71, fmv.x.s = funct7 0x70.
                let funct7 = if f64 { 0x71 } else { 0x70 };
                self.insn(encode_fp(funct7, 0, reg, fs, 0, 0, 0));
                Ok(())
            }
            PhysLoc::Slot(_) => self.store_fslot(fs, vreg, f64),
        }
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
        // RISC-V has no 32-bit forms of AND/OR/XOR: OP-32 with funct3
        // 4/6/7 and funct7 0 is an unassigned encoding that raises an
        // illegal-instruction fault on hardware, not just under
        // emulation (#161). The W32 vreg loads zero-extend, so the
        // 64-bit form computes the same low 32 bits and the 32-bit
        // store discards the rest.
        let opcode = match width {
            Width::W32 if funct7 == 0 && matches!(funct3, 4 | 6 | 7) => 0x33,
            Width::W32 => 0x3b,
            Width::X64 => 0x33,
        };
        // #193: register-homed operands feed the op directly from
        // their home registers; only slot-resident operands are
        // staged through T0/T1. A register-homed destination is
        // computed in place (a single R-format instruction reads both
        // sources before writing, so aliasing is safe); a
        // slot-resident destination accumulates in T0, or T2 when T0
        // already holds the staged left operand.
        let rs1 = match Self::phys_loc(rn).expect("virtual register fits") {
            PhysLoc::Reg(reg) => reg,
            PhysLoc::Slot(_) => {
                match width {
                    Width::W32 => self.load_vreg32(T0, rn),
                    Width::X64 => self.load_vreg64(T0, rn),
                }
                .expect("virtual register fits");
                T0
            }
        };
        let rs2 = match Self::phys_loc(rm).expect("virtual register fits") {
            PhysLoc::Reg(reg) => reg,
            PhysLoc::Slot(_) => {
                match width {
                    Width::W32 => self.load_vreg32(T1, rm),
                    Width::X64 => self.load_vreg64(T1, rm),
                }
                .expect("virtual register fits");
                T1
            }
        };
        match Self::phys_loc(rd).expect("virtual register fits") {
            PhysLoc::Reg(home) => {
                self.insn(encode_r(opcode, funct3, funct7, home, rs1, rs2));
                if width == Width::W32 {
                    // OP-32 results are sign-extended by the ISA and
                    // the 64-bit logic forms propagate operand upper
                    // bits; canonicalise the home back to the
                    // zero-extended form every 32-bit value keeps.
                    self.slli(home, home, 32);
                    self.srli(home, home, 32);
                }
            }
            PhysLoc::Slot(_) => {
                let acc = if rs1 == T0 { T2 } else { T0 };
                self.insn(encode_r(opcode, funct3, funct7, acc, rs1, rs2));
                match width {
                    Width::W32 => self.store_vreg32(rd, acc),
                    Width::X64 => self.store_vreg64(rd, acc),
                }
                .expect("virtual register fits");
            }
        }
    }

    fn rotate_right(&mut self, rd: u8, rn: u8, rm: u8, width: Width) {
        match width {
            Width::W32 => {
                self.load_vreg32(T0, rn).expect("virtual register fits");
                self.load_vreg32(T1, rm).expect("virtual register fits");
                self.insn(encode_r(0x3b, 5, 0, T2, T0, T1));
                self.insn(encode_r(0x3b, 0, 0x20, T1, ZERO, T1));
                self.insn(encode_r(0x3b, 1, 0, T0, T0, T1));
                self.insn(encode_r(0x33, 6, 0, T0, T0, T2));
                self.store_vreg32(rd, T0).expect("virtual register fits");
            }
            Width::X64 => {
                self.load_vreg64(T0, rn).expect("virtual register fits");
                self.load_vreg64(T1, rm).expect("virtual register fits");
                self.insn(encode_r(0x33, 5, 0, T2, T0, T1));
                self.insn(encode_r(0x33, 0, 0x20, T1, ZERO, T1));
                self.insn(encode_r(0x33, 1, 0, T0, T0, T1));
                self.insn(encode_r(0x33, 6, 0, T0, T0, T2));
                self.store_vreg64(rd, T0).expect("virtual register fits");
            }
        }
    }

    fn clz(&mut self, rd: u8, rn: u8, width: Width) {
        let bits = match width {
            Width::W32 => 32,
            Width::X64 => 64,
        };
        self.load_vreg_width(T0, rn, width)
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
        self.store_vreg32(rd, T1).expect("virtual register fits");
    }

    fn ctz(&mut self, rd: u8, rn: u8, width: Width) {
        let bits = match width {
            Width::W32 => 32,
            Width::X64 => 64,
        };
        self.load_vreg_width(T0, rn, width)
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
        self.store_vreg32(rd, T1).expect("virtual register fits");
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
                self.load_vreg_width(T0, lhs, width)?;
                match rhs {
                    Rhs::Reg(reg) => self.load_vreg_width(T1, reg, width)?,
                    Rhs::Imm32(imm) => self.li(T1, i64::from(imm as i32)),
                }
                Ok((width, None))
            }
            LastCmp::Float { width, lhs, rhs } => {
                self.load_fvreg(0, lhs, width == FloatWidth::F64)?;
                self.load_fvreg(1, rhs, width == FloatWidth::F64)?;
                Ok((Width::X64, Some(width)))
            }
        }
    }

    fn emit_cmp_bool(&mut self, dst: u8, cmp: LastCmp, cond: Cond) -> AsmResult {
        let (width, float) = self.load_cmp_operands(cmp)?;
        if let Some(float_width) = float {
            return self.emit_float_cmp_bool(dst, float_width, cond);
        }
        if width == Width::W32 && cond.uses_signed_operands() {
            self.slli(T0, T0, 32);
            self.srai(T0, T0, 32);
            self.slli(T1, T1, 32);
            self.srai(T1, T1, 32);
        } else if width == Width::W32 {
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
        // The skip lands just past the eight-byte branch pair that
        // follows (and the four-byte instruction this branch itself
        // occupies), hence 12 from this branch's own PC.
        self.insn(encode_b(0b001, T4, ZERO, 12).expect("local branch fits"));
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
        self.load_fvreg(0, vn, f64).expect("virtual register fits");
        self.load_fvreg(1, vm, f64).expect("virtual register fits");
        self.insn(encode_fp(funct7, rm, 0, 0, 1, 0, 0));
        self.store_fvreg(0, vd, f64).expect("virtual register fits");
    }

    fn float_unop(&mut self, vd: u8, vn: u8, f64: bool, funct7: u32, rm: u32) {
        self.load_fvreg(0, vn, f64).expect("virtual register fits");
        self.insn(encode_fp(funct7, rm, 0, 0, 0, 0, 0));
        self.store_fvreg(0, vd, f64).expect("virtual register fits");
    }

    fn round(&mut self, _vd: u8, _vn: u8, _f64: bool, _rm: u32) -> AsmResult {
        // Base RISC-V F/D has no float-to-float rounding instruction. The
        // previous fcvt.l round-trip changed NaNs, infinities, and large
        // finite values, so callers must use a semantic helper instead.
        Err(AsmError::UnsupportedFeature)
    }

    fn cvt_int_to_float(&mut self, vd: u8, rn: u8, to_f64: bool, from_i64: bool, signed: bool) {
        if from_i64 {
            self.load_vreg64(T0, rn).expect("virtual register fits");
        } else if signed {
            self.load_vreg32(T0, rn).expect("virtual register fits");
            self.slli(T0, T0, 32);
            self.srai(T0, T0, 32);
        } else {
            self.load_vreg32(T0, rn).expect("virtual register fits");
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
        self.store_fvreg(0, vd, to_f64)
            .expect("virtual register fits");
    }

    fn fcvt_w(&mut self, rd: u8, vn: u8, f64: bool, signed: bool) {
        self.load_fvreg(0, vn, f64).expect("virtual register fits");
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
        self.store_vreg32(rd, T0).expect("virtual register fits");
    }

    fn load_mem_to_slot(&mut self, rt: u8, rn: u8, width: u32, signed: bool) {
        self.load_vreg64(T0, rn).expect("virtual register fits");
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
        self.store_vreg32(rt, T1).expect("virtual register fits");
    }

    fn store_slot_to_mem(&mut self, rt: u8, rn: u8, width: u32) {
        self.load_vreg64(T0, rn).expect("virtual register fits");
        self.load_vreg32(T1, rt).expect("virtual register fits");
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

crate::impl_baseline_masm!(Riscv64BaselineMasm);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csel_and_cset_reuse_one_compare() {
        // Regression test for #309: materialising the deferred compare
        // must not consume it. A multi-lane select emits one compare
        // followed by one csel per lane; the second csel used to panic
        // with "csel follows cmp".
        let mut asm = Riscv64BaselineMasm::with_capacity(256);
        asm.cmp_w_imm(0, 0).unwrap();
        asm.csel_w(1, 1, 2, Cond::Ne);
        asm.csel_w(2, 1, 2, Cond::Ne);
        asm.cset_w(3, Cond::Ne);
        assert!(!asm.into_bytes().is_empty());
    }

    /// Tiny interpreter for the two instruction forms `li` / `li_reloc` /
    /// `li_reloc32` emit (addi and slli into a single register), used to
    /// check that a materialised immediate equals the requested value.
    fn eval_li_sequence(bytes: &[u8], rd: u8) -> i64 {
        let mut regs = [0i64; 32];
        for word in bytes.chunks_exact(4) {
            let insn = u32::from_le_bytes(word.try_into().unwrap());
            assert_eq!(insn & 0x7f, 0x13, "li sequences only use OP-IMM");
            let dest = ((insn >> 7) & 0x1f) as usize;
            let rs1 = ((insn >> 15) & 0x1f) as usize;
            let imm = i64::from((insn as i32) >> 20);
            match (insn >> 12) & 0x7 {
                0 => regs[dest] = regs[rs1].wrapping_add(imm),
                1 => regs[dest] = regs[rs1] << ((insn >> 20) & 0x3f),
                funct3 => panic!("unexpected funct3 {funct3} in li sequence"),
            }
        }
        regs[usize::from(rd)]
    }

    #[test]
    fn li_handles_immediates_near_i64_max() {
        // Regression test for #328: the 12-bit chunk decomposition
        // computed `value - low` with plain subtraction. When the
        // sign-extended low chunk is negative and the value is near
        // i64::MAX, that subtraction overflows and panics in debug
        // builds (release builds wrapped and produced the right code).
        for value in [
            i64::MAX,
            i64::MAX - 1,
            i64::MIN,
            i64::from(i32::MAX),
            i64::from(i32::MIN),
            -1,
            0,
            2047,
            2048,
            -2048,
            -2049,
            1 << 62,
            -(1 << 62),
            0x1234_5678_9abc_def0u64 as i64,
        ] {
            let mut asm = Riscv64BaselineMasm::with_capacity(256);
            asm.li(A0, value);
            assert_eq!(
                eval_li_sequence(&asm.into_bytes(), A0),
                value,
                "li({value})"
            );
            let mut asm = Riscv64BaselineMasm::with_capacity(256);
            asm.li_reloc(A0, value);
            assert_eq!(
                eval_li_sequence(&asm.into_bytes(), A0),
                value,
                "li_reloc({value})"
            );
        }
        for value in [i64::from(i32::MAX), i64::from(i32::MIN), -1, 0, 2047, -2048] {
            let mut asm = Riscv64BaselineMasm::with_capacity(256);
            asm.li_reloc32(A0, value);
            assert_eq!(
                eval_li_sequence(&asm.into_bytes(), A0),
                value,
                "li_reloc32({value})"
            );
        }
    }

    /// Decodes the `auipc`/`jalr` branch pair at `at` and returns
    /// the absolute target it jumps to.
    fn decode_branch_pair(bytes: &[u8], at: usize) -> usize {
        let auipc = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        let jalr = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap());
        assert_eq!(auipc & 0x7f, 0x17, "first word must be auipc");
        assert_eq!((auipc >> 7) & 0x1f, T0 as u32, "auipc writes T0");
        assert_eq!(jalr & 0x7f, 0x67, "second word must be jalr");
        assert_eq!((jalr >> 7) & 0x1f, 0, "jalr discards the link");
        assert_eq!((jalr >> 15) & 0x1f, T0 as u32, "jalr reads T0");
        let hi = (((auipc >> 12) & 0x000f_ffff) << 12) as i32;
        let lo = (jalr as i32) >> 20;
        (at as i64 + i64::from(hi) + i64::from(lo)) as usize
    }

    #[test]
    fn patches_branch_forward() {
        let mut bytes = vec![0u8; 8];
        patch_branch(&mut bytes, 0, 16, BranchKind::B).unwrap();
        assert_eq!(decode_branch_pair(&bytes, 0), 16);
    }

    #[test]
    fn patches_branch_beyond_jal_range() {
        // Regression test for #310: the epilogue fixups of the
        // 3,001-function cache test's `run` function and of
        // skip-stack-guard-page's many-locals function sit more than
        // the ±1 MiB jal range away from their target (measured
        // deltas 1,631,100 and 1,371,144 bytes), so patching them
        // with a lone jal failed and the functions were rejected.
        for (at, target) in [(180usize, 1_631_280usize), (272, 1_371_416), (0, 2_000_000)] {
            let mut bytes = vec![0u8; target.max(at) + 8];
            patch_branch(&mut bytes, at, target, BranchKind::B).unwrap();
            assert_eq!(decode_branch_pair(&bytes, at), target);
        }
        // Backwards past the jal range as well.
        let mut bytes = vec![0u8; 2_000_008];
        patch_branch(&mut bytes, 2_000_000, 180, BranchKind::B).unwrap();
        assert_eq!(decode_branch_pair(&bytes, 2_000_000), 180);
        // Past even the pair's ±2 GiB range the patch still fails
        // closed instead of encoding a wrong target.
        let mut bytes = vec![0u8; 8];
        assert!(patch_branch(&mut bytes, 0, (1usize << 31) + 16, BranchKind::B).is_err());
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
        // Slot-resident operands (vregs 29-31 have no register home,
        // #193) stage through T0/T1 and accumulate in T2.
        let mut asm = Riscv64BaselineMasm::with_capacity(32);
        asm.add_w(31, 29, 30);
        let bytes = asm.into_bytes();
        assert!(bytes
            .windows(4)
            .any(|window| window == encode_r(0x3b, 0, 0, T2, T0, T1).to_le_bytes()));
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

    #[test]
    fn emits_fcvt_s_from_d_with_double_source_format() {
        let mut asm = Riscv64BaselineMasm::with_capacity(64);
        asm.fcvt_s_from_d(2, 1);
        assert!(asm
            .into_bytes()
            .windows(4)
            .any(|window| u32::from_le_bytes(window.try_into().unwrap())
                == encode_fp(0x20, 0, 0, 0, 1, 1, 0)));
    }

    #[test]
    fn rejects_integer_round_trip_float_rounding() {
        let mut asm = Riscv64BaselineMasm::with_capacity(64);
        assert_eq!(asm.frintp_s(0, 1), Err(AsmError::UnsupportedFeature));
        assert!(asm.into_bytes().is_empty());
    }

    #[test]
    fn sign_extends_riscv32_signed_compare_operands() {
        let mut asm = Riscv64BaselineMasm::with_capacity(128);
        asm.emit_cmp_bool(
            T2,
            LastCmp::Int {
                width: Width::W32,
                lhs: 0,
                rhs: Rhs::Reg(1),
            },
            Cond::Lt,
        )
        .unwrap();
        let bytes = asm.into_bytes();

        assert!(bytes.windows(4).any(|window| {
            u32::from_le_bytes(window.try_into().unwrap())
                == encode_i(0x13, 5, T0, T0, (0x20 << 5) | 32).unwrap()
        }));
        assert!(bytes.windows(4).any(|window| {
            u32::from_le_bytes(window.try_into().unwrap())
                == encode_i(0x13, 5, T1, T1, (0x20 << 5) | 32).unwrap()
        }));
        // The sign extension must be the LAST shift applied to each
        // operand: a 32-bit load from a register home (#193)
        // legitimately zero-extends with srli on the way in, so the
        // property is about which extension the compare finally
        // sees, not about srli never appearing.
        for reg in [T0, T1] {
            let last_shift = bytes
                .chunks_exact(4)
                .map(|w| u32::from_le_bytes(w.try_into().unwrap()))
                .rfind(|&w| {
                    w & 0x7f == 0x13
                        && (w >> 12) & 0x7 == 5
                        && (w >> 7) & 0x1f == u32::from(reg)
                        && (w >> 15) & 0x1f == u32::from(reg)
                })
                .expect("compare emits a right shift on each operand");
            assert_eq!(
                last_shift,
                encode_i(0x13, 5, reg, reg, (0x20 << 5) | 32).unwrap(),
                "operand {reg} must be sign-extended last"
            );
        }
    }

    #[test]
    fn prologue_slots_do_not_overlap_saved_registers() {
        // #161: the slot base used to be anchored at the frame top,
        // putting slot 0 on the RA spill and slot 1 on the S0 spill.
        // The entry argument spill then overwrote the saved return
        // address, and the epilogue returned to whatever value the
        // function body last left in slot 0.
        let mut asm = Riscv64BaselineMasm::with_capacity(256);
        asm.stp_pre(29, 30);
        let bytes = asm.into_bytes();
        let words: Vec<u32> = bytes
            .chunks_exact(4)
            .map(|w| u32::from_le_bytes(w.try_into().unwrap()))
            .collect();

        // Recover the slot base from the addi that loads S0 from SP.
        let slot_base = words
            .iter()
            .filter(|&&w| {
                w & 0x7f == 0x13
                    && (w >> 7) & 0x1f == u32::from(S0)
                    && (w >> 15) & 0x1f == u32::from(SP)
                    && (w >> 12) & 0x7 == 0
            })
            .map(|&w| i64::from((w as i32) >> 20))
            .next()
            .expect("prologue sets the slot base from SP");

        // Effective frame offsets (from the new SP) of every 64-bit
        // store the prologue emits, split by base register.
        let mut saved_offsets = Vec::new();
        let mut slot_offsets = Vec::new();
        for &w in &words {
            if w & 0x7f != 0x23 || (w >> 12) & 0x7 != 3 {
                continue; // not an sd
            }
            let base = (w >> 15) & 0x1f;
            let imm = (((w as i32) >> 25) << 5) | ((w >> 7) & 0x1f) as i32;
            if base == u32::from(SP) {
                saved_offsets.push(i64::from(imm));
            } else if base == u32::from(S0) {
                slot_offsets.push(slot_base + i64::from(imm));
            }
        }
        assert_eq!(saved_offsets.len(), 2, "prologue spills RA and S0");
        // #193: the arguments arrive in their home registers (A0-A7
        // are vregs 0-7), so instead of argument spills the prologue
        // preserves the eleven callee-saved home registers (S1-S11) into
        // the frame slots of the vregs they back (19-28).
        assert_eq!(
            slot_offsets.len(),
            11,
            "prologue preserves eleven callee-saved homes"
        );
        for slot in &slot_offsets {
            assert!(
                !saved_offsets.contains(slot),
                "slot at frame offset {slot} overlaps a saved-register spill {saved_offsets:?}"
            );
            assert!((0..i64::from(FRAME_SIZE)).contains(slot));
        }
    }

    #[test]
    fn register_mapping_is_disjoint_from_encoder_scratch() {
        // #193: no virtual register may be homed in a register the
        // encoder itself uses as transient scratch (T0-T4), in the
        // frame base S0, or in SP/RA/ZERO, and two vregs never share
        // a home.
        let mut homes = std::collections::HashSet::new();
        for vreg in 0..32u8 {
            if let PhysLoc::Reg(reg) = Riscv64BaselineMasm::phys_loc(vreg).unwrap() {
                assert!(
                    !matches!(reg, ZERO | RA | SP | T0 | T1 | T2 | S0 | T3 | T4),
                    "vreg {vreg} homed in encoder scratch {reg}"
                );
                assert!(homes.insert(reg), "vreg {vreg} shares home {reg}");
            }
        }
        // Arguments are already home; the call-preserved set is
        // homed in callee-saved registers.
        for vreg in 0..8u8 {
            assert_eq!(
                Riscv64BaselineMasm::phys_loc(vreg).unwrap(),
                PhysLoc::Reg(A0 + vreg)
            );
        }
        for vreg in [11u8, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28] {
            let PhysLoc::Reg(reg) = Riscv64BaselineMasm::phys_loc(vreg).unwrap() else {
                panic!("vreg {vreg} must be register-homed");
            };
            assert!(
                matches!(reg, S1 | S2 | S3 | S4 | S5 | S6 | S7 | S8 | S9 | S10 | S11),
                "vreg {vreg} homed in caller-saved {reg}"
            );
        }
    }

    #[test]
    fn emits_addw_between_argument_registers() {
        // #193: add_w over argument vregs runs in the destination's
        // home register — no frame slot is touched. The trailing
        // shift pair canonicalises the OP-32 result back to the
        // zero-extended form 32-bit values keep.
        let mut asm = Riscv64BaselineMasm::with_capacity(32);
        asm.add_w(2, 0, 1);
        let mut expected = encode_r(0x3b, 0, 0, A2, A0, A1).to_le_bytes().to_vec();
        expected.extend_from_slice(&encode_i(0x13, 1, A2, A2, 32).unwrap().to_le_bytes());
        expected.extend_from_slice(&encode_i(0x13, 5, A2, A2, 32).unwrap().to_le_bytes());
        assert_eq!(asm.into_bytes(), expected);
    }

    #[test]
    fn emits_bitwise_w_with_64_bit_forms() {
        // #161: and_w/orr_w/eor_w must not emit the unassigned OP-32
        // encodings (funct3 4/6/7, funct7 0); the 64-bit forms are
        // used instead.
        let emit: [fn(&mut Riscv64BaselineMasm, u8, u8, u8); 3] = [
            Riscv64BaselineMasm::and_w,
            Riscv64BaselineMasm::orr_w,
            Riscv64BaselineMasm::eor_w,
        ];
        for (emit, funct3) in emit.into_iter().zip([7, 6, 4]) {
            let mut asm = Riscv64BaselineMasm::with_capacity(64);
            emit(&mut asm, 2, 0, 1);
            let bytes = asm.into_bytes();
            // #193: vregs 0-2 are register-homed, so the 64-bit form
            // is emitted in the destination's home register.
            assert!(bytes
                .windows(4)
                .any(|window| { window == encode_r(0x33, funct3, 0, A2, A0, A1).to_le_bytes() }));
            assert!(!bytes
                .windows(4)
                .any(|window| { window == encode_r(0x3b, funct3, 0, T0, T0, T1).to_le_bytes() }));
        }
    }
}
