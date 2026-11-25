#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsmError {
    InvalidRegister,
    InvalidImmediate,
    BranchOutOfRange,
    UnsupportedFeature,
}

impl From<AsmError> for () {
    fn from(error: AsmError) -> Self {
        if std::env::var_os("TELOMERE_JIT_TRACE_COMPILE").is_some() {
            eprintln!("[telomere-jit] assembler error: {error:?}");
        }
    }
}

pub type AsmResult<T = ()> = Result<T, AsmError>;

/// A condition code, in target-independent semantic form. Each target
/// encoder maps it to its own encoding (see the `cond_code` helpers in
/// the `arch` modules); the discriminants here carry no meaning.
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

    pub const fn uses_signed_operands(self) -> bool {
        matches!(self, Self::Ge | Self::Gt | Self::Le | Self::Lt)
    }
}

/// The placeholder branch shapes the lowering can request. `CbzW` is
/// only encodable on aarch64; the other encoders reject it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BranchKind {
    B,
    BCond(Cond),
    CbnzX(u8),
    CbnzW(u8),
    CbzW(u8),
}

/// The shared macro-assembler surface the baseline JIT lowering is
/// written against. All three target encoders implement exactly this
/// interface; the lowering in the core crate is generic over it, so a
/// new target costs one implementation of this trait (plus its
/// `TargetHooks` in the core crate) instead of a copied lowering.
///
/// `frint*_s`/`frint*_d` return `AsmResult` because some encoders can
/// only emit them when a host feature is present (x86_64 SSE4.1);
/// encoders that always can simply return `Ok(())`.
pub trait BaselineMasm: Sized {
    fn with_capacity(capacity: usize) -> Self;
    fn offset(&self) -> usize;
    fn truncate(&mut self, len: usize);
    fn as_mut_bytes(&mut self) -> &mut [u8];
    fn into_bytes(self) -> Vec<u8>;
    fn branch_placeholder(&mut self, kind: BranchKind) -> AsmResult<usize>;
    fn ret(&mut self);
    fn mov_x_from_sp(&mut self, rd: u8);
    fn blr_x(&mut self, rn: u8);
    fn mov_x(&mut self, rd: u8, rn: u8);
    fn mov_w(&mut self, rd: u8, rn: u8);
    fn mov_imm_u32(&mut self, rd: u8, value: u32);
    fn mov_imm_u64(&mut self, rd: u8, value: u64);
    fn mov_imm_u32_reloc(&mut self, rd: u8, value: u32);
    fn mov_imm_u64_reloc(&mut self, rd: u8, value: u64);
    fn stp_pre_x_sp(&mut self, rt: u8, rt2: u8);
    fn stp_pre(&mut self, rt: u8, rt2: u8);
    fn ldp_post_x_sp(&mut self, rt: u8, rt2: u8);
    fn ldp_post(&mut self, rt: u8, rt2: u8);
    fn add_imm_u32(&mut self, rd: u8, rn: u8, imm: u32) -> AsmResult;
    fn add_imm_u64(&mut self, rd: u8, rn: u8, imm: u64) -> AsmResult;
    fn sub_imm_u32(&mut self, rd: u8, rn: u8, imm: u32) -> AsmResult;
    fn add_w(&mut self, rd: u8, rn: u8, rm: u8);
    fn add_x(&mut self, rd: u8, rn: u8, rm: u8);
    fn sub_w(&mut self, rd: u8, rn: u8, rm: u8);
    fn sub_x(&mut self, rd: u8, rn: u8, rm: u8);
    fn mul_w(&mut self, rd: u8, rn: u8, rm: u8);
    fn mul_x(&mut self, rd: u8, rn: u8, rm: u8);
    fn udiv_w(&mut self, rd: u8, rn: u8, rm: u8);
    fn sdiv_w(&mut self, rd: u8, rn: u8, rm: u8);
    fn udiv_x(&mut self, rd: u8, rn: u8, rm: u8);
    fn sdiv_x(&mut self, rd: u8, rn: u8, rm: u8);
    fn msub_w(&mut self, rd: u8, rn: u8, rm: u8, ra: u8);
    fn msub_x(&mut self, rd: u8, rn: u8, rm: u8, ra: u8);
    fn neg_w(&mut self, rd: u8, rm: u8);
    fn neg_x(&mut self, rd: u8, rm: u8);
    fn and_w(&mut self, rd: u8, rn: u8, rm: u8);
    fn and_x(&mut self, rd: u8, rn: u8, rm: u8);
    fn orr_w(&mut self, rd: u8, rn: u8, rm: u8);
    fn orr_x(&mut self, rd: u8, rn: u8, rm: u8);
    fn eor_w(&mut self, rd: u8, rn: u8, rm: u8);
    fn eor_x(&mut self, rd: u8, rn: u8, rm: u8);
    fn lslv_w(&mut self, rd: u8, rn: u8, rm: u8);
    fn lslv_x(&mut self, rd: u8, rn: u8, rm: u8);
    fn lsrv_w(&mut self, rd: u8, rn: u8, rm: u8);
    fn lsrv_x(&mut self, rd: u8, rn: u8, rm: u8);
    fn asrv_w(&mut self, rd: u8, rn: u8, rm: u8);
    fn asrv_x(&mut self, rd: u8, rn: u8, rm: u8);
    fn rorv_w(&mut self, rd: u8, rn: u8, rm: u8);
    fn rorv_x(&mut self, rd: u8, rn: u8, rm: u8);
    fn rbit_w(&mut self, rd: u8, rn: u8);
    fn rbit_x(&mut self, rd: u8, rn: u8);
    fn clz_w(&mut self, rd: u8, rn: u8);
    fn clz_x(&mut self, rd: u8, rn: u8);
    fn sxth_w(&mut self, rd: u8, rn: u8);
    fn sxtb_w(&mut self, rd: u8, rn: u8);
    fn cmp_w(&mut self, rn: u8, rm: u8);
    fn cmp_x(&mut self, rn: u8, rm: u8);
    fn cmp_w_imm(&mut self, rn: u8, imm: u32) -> AsmResult;
    fn cmp_w_u32(&mut self, rn: u8, imm: u32);
    fn cset_w(&mut self, rd: u8, cond: Cond);
    fn csel_w(&mut self, rd: u8, rn: u8, rm: u8, cond: Cond);
    fn fmov_s_from_w(&mut self, vd: u8, rn: u8);
    fn fmov_w_from_s(&mut self, rd: u8, vn: u8);
    fn fmov_d_from_x(&mut self, vd: u8, rn: u8);
    fn fmov_x_from_d(&mut self, rd: u8, vn: u8);
    fn fcmp_s(&mut self, vn: u8, vm: u8);
    fn fcmp_d(&mut self, vn: u8, vm: u8);
    fn fadd_d(&mut self, vd: u8, vn: u8, vm: u8);
    fn fadd_s(&mut self, vd: u8, vn: u8, vm: u8);
    fn fsub_d(&mut self, vd: u8, vn: u8, vm: u8);
    fn fsub_s(&mut self, vd: u8, vn: u8, vm: u8);
    fn fmul_d(&mut self, vd: u8, vn: u8, vm: u8);
    fn fmul_s(&mut self, vd: u8, vn: u8, vm: u8);
    fn fdiv_d(&mut self, vd: u8, vn: u8, vm: u8);
    fn fdiv_s(&mut self, vd: u8, vn: u8, vm: u8);
    fn fabs_s(&mut self, vd: u8, vn: u8);
    fn fabs_d(&mut self, vd: u8, vn: u8);
    fn fneg_s(&mut self, vd: u8, vn: u8);
    fn fneg_d(&mut self, vd: u8, vn: u8);
    fn fsqrt_s(&mut self, vd: u8, vn: u8);
    fn fsqrt_d(&mut self, vd: u8, vn: u8);
    fn frintn_s(&mut self, vd: u8, vn: u8) -> AsmResult;
    fn frintn_d(&mut self, vd: u8, vn: u8) -> AsmResult;
    fn frintp_s(&mut self, vd: u8, vn: u8) -> AsmResult;
    fn frintp_d(&mut self, vd: u8, vn: u8) -> AsmResult;
    fn frintm_s(&mut self, vd: u8, vn: u8) -> AsmResult;
    fn frintm_d(&mut self, vd: u8, vn: u8) -> AsmResult;
    fn frintz_s(&mut self, vd: u8, vn: u8) -> AsmResult;
    fn frintz_d(&mut self, vd: u8, vn: u8) -> AsmResult;
    fn cvtf_d_from_w(&mut self, vd: u8, rn: u8, signed: bool);
    fn cvtf_s_from_w(&mut self, vd: u8, rn: u8, signed: bool);
    fn cvtf_d_from_x(&mut self, vd: u8, rn: u8, signed: bool);
    fn cvtf_s_from_x(&mut self, vd: u8, rn: u8, signed: bool);
    fn fcvt_s_from_d(&mut self, vd: u8, vn: u8);
    fn fcvt_d_from_s(&mut self, vd: u8, vn: u8);
    fn fcvt_w_from_s(&mut self, rd: u8, vn: u8, signed: bool);
    fn fcvt_w_from_d(&mut self, rd: u8, vn: u8, signed: bool);
    fn lsl_w_imm(&mut self, rd: u8, rn: u8, shift: u32);
    fn lsr_w_imm(&mut self, rd: u8, rn: u8, shift: u32);
    fn asr_w_imm(&mut self, rd: u8, rn: u8, shift: u32);
    fn ubfm_w(&mut self, rd: u8, rn: u8, immr: u32, imms: u32);
    fn ubfm_x(&mut self, rd: u8, rn: u8, immr: u32, imms: u32);
    fn sbfm_w(&mut self, rd: u8, rn: u8, immr: u32, imms: u32);
    fn sbfm_x(&mut self, rd: u8, rn: u8, immr: u32, imms: u32);
    fn lsl_x_imm(&mut self, rd: u8, rn: u8, shift: u32) -> AsmResult;
    fn lsr_x_imm(&mut self, rd: u8, rn: u8, shift: u32) -> AsmResult;
    fn ldr_x_imm(&mut self, rt: u8, rn: u8, offset: usize) -> AsmResult;
    fn ldr_w_imm(&mut self, rt: u8, rn: u8, offset: usize) -> AsmResult;
    fn ldr_w_unscaled_imm(&mut self, rt: u8, rn: u8, offset: u32) -> AsmResult;
    fn ldrb_w(&mut self, rt: u8, rn: u8);
    fn ldrsb_w(&mut self, rt: u8, rn: u8);
    fn ldrh_w(&mut self, rt: u8, rn: u8);
    fn ldrsh_w(&mut self, rt: u8, rn: u8);
    fn ldr_w(&mut self, rt: u8, rn: u8);
    fn strb_w(&mut self, rt: u8, rn: u8);
    fn strh_w(&mut self, rt: u8, rn: u8);
    fn str_w(&mut self, rt: u8, rn: u8);
    fn str_x_imm(&mut self, rt: u8, rn: u8, offset: usize) -> AsmResult;
    fn str_w_imm(&mut self, rt: u8, rn: u8, offset: usize) -> AsmResult;
    fn str_w_unscaled_imm(&mut self, rt: u8, rn: u8, offset: u32) -> AsmResult;

    /// Patches a placeholder branch emitted by `branch_placeholder`
    /// (or an identical placeholder sequence) at byte offset `at` to
    /// target byte offset `target`. Delegates to the target module's
    /// free `patch_branch` function.
    fn patch_branch(bytes: &mut [u8], at: usize, target: usize, kind: BranchKind) -> AsmResult;
}
