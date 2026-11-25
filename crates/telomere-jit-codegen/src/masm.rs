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

/// Generates the `BaselineMasm` implementation for one target's
/// macro assembler by delegating every trait method to the
/// inherent method of the same name. The inherent methods are the
/// per-target encoders; this macro exists so the delegation list
/// (120+ methods) is written exactly once.
#[macro_export]
macro_rules! impl_baseline_masm {
    ($ty:ty) => {
        impl $crate::masm::BaselineMasm for $ty {
            fn with_capacity(capacity: usize) -> Self {
                <$ty>::with_capacity(capacity)
            }
            fn offset(&self) -> usize {
                <$ty>::offset(self)
            }
            fn truncate(&mut self, len: usize) {
                <$ty>::truncate(self, len)
            }
            fn as_mut_bytes(&mut self) -> &mut [u8] {
                <$ty>::as_mut_bytes(self)
            }
            fn into_bytes(self) -> Vec<u8> {
                <$ty>::into_bytes(self)
            }
            fn branch_placeholder(
                &mut self,
                kind: $crate::masm::BranchKind,
            ) -> $crate::masm::AsmResult<usize> {
                <$ty>::branch_placeholder(self, kind)
            }
            fn ret(&mut self) {
                <$ty>::ret(self)
            }
            fn mov_x_from_sp(&mut self, rd: u8) {
                <$ty>::mov_x_from_sp(self, rd)
            }
            fn blr_x(&mut self, rn: u8) {
                <$ty>::blr_x(self, rn)
            }
            fn mov_x(&mut self, rd: u8, rn: u8) {
                <$ty>::mov_x(self, rd, rn)
            }
            fn mov_w(&mut self, rd: u8, rn: u8) {
                <$ty>::mov_w(self, rd, rn)
            }
            fn mov_imm_u32(&mut self, rd: u8, value: u32) {
                <$ty>::mov_imm_u32(self, rd, value)
            }
            fn mov_imm_u64(&mut self, rd: u8, value: u64) {
                <$ty>::mov_imm_u64(self, rd, value)
            }
            fn mov_imm_u32_reloc(&mut self, rd: u8, value: u32) {
                <$ty>::mov_imm_u32_reloc(self, rd, value)
            }
            fn mov_imm_u64_reloc(&mut self, rd: u8, value: u64) {
                <$ty>::mov_imm_u64_reloc(self, rd, value)
            }
            fn stp_pre_x_sp(&mut self, rt: u8, rt2: u8) {
                <$ty>::stp_pre_x_sp(self, rt, rt2)
            }
            fn stp_pre(&mut self, rt: u8, rt2: u8) {
                <$ty>::stp_pre(self, rt, rt2)
            }
            fn ldp_post_x_sp(&mut self, rt: u8, rt2: u8) {
                <$ty>::ldp_post_x_sp(self, rt, rt2)
            }
            fn ldp_post(&mut self, rt: u8, rt2: u8) {
                <$ty>::ldp_post(self, rt, rt2)
            }
            fn add_imm_u32(&mut self, rd: u8, rn: u8, imm: u32) -> $crate::masm::AsmResult {
                <$ty>::add_imm_u32(self, rd, rn, imm)
            }
            fn add_imm_u64(&mut self, rd: u8, rn: u8, imm: u64) -> $crate::masm::AsmResult {
                <$ty>::add_imm_u64(self, rd, rn, imm)
            }
            fn sub_imm_u32(&mut self, rd: u8, rn: u8, imm: u32) -> $crate::masm::AsmResult {
                <$ty>::sub_imm_u32(self, rd, rn, imm)
            }
            fn add_w(&mut self, rd: u8, rn: u8, rm: u8) {
                <$ty>::add_w(self, rd, rn, rm)
            }
            fn add_x(&mut self, rd: u8, rn: u8, rm: u8) {
                <$ty>::add_x(self, rd, rn, rm)
            }
            fn sub_w(&mut self, rd: u8, rn: u8, rm: u8) {
                <$ty>::sub_w(self, rd, rn, rm)
            }
            fn sub_x(&mut self, rd: u8, rn: u8, rm: u8) {
                <$ty>::sub_x(self, rd, rn, rm)
            }
            fn mul_w(&mut self, rd: u8, rn: u8, rm: u8) {
                <$ty>::mul_w(self, rd, rn, rm)
            }
            fn mul_x(&mut self, rd: u8, rn: u8, rm: u8) {
                <$ty>::mul_x(self, rd, rn, rm)
            }
            fn udiv_w(&mut self, rd: u8, rn: u8, rm: u8) {
                <$ty>::udiv_w(self, rd, rn, rm)
            }
            fn sdiv_w(&mut self, rd: u8, rn: u8, rm: u8) {
                <$ty>::sdiv_w(self, rd, rn, rm)
            }
            fn udiv_x(&mut self, rd: u8, rn: u8, rm: u8) {
                <$ty>::udiv_x(self, rd, rn, rm)
            }
            fn sdiv_x(&mut self, rd: u8, rn: u8, rm: u8) {
                <$ty>::sdiv_x(self, rd, rn, rm)
            }
            fn msub_w(&mut self, rd: u8, rn: u8, rm: u8, ra: u8) {
                <$ty>::msub_w(self, rd, rn, rm, ra)
            }
            fn msub_x(&mut self, rd: u8, rn: u8, rm: u8, ra: u8) {
                <$ty>::msub_x(self, rd, rn, rm, ra)
            }
            fn neg_w(&mut self, rd: u8, rm: u8) {
                <$ty>::neg_w(self, rd, rm)
            }
            fn neg_x(&mut self, rd: u8, rm: u8) {
                <$ty>::neg_x(self, rd, rm)
            }
            fn and_w(&mut self, rd: u8, rn: u8, rm: u8) {
                <$ty>::and_w(self, rd, rn, rm)
            }
            fn and_x(&mut self, rd: u8, rn: u8, rm: u8) {
                <$ty>::and_x(self, rd, rn, rm)
            }
            fn orr_w(&mut self, rd: u8, rn: u8, rm: u8) {
                <$ty>::orr_w(self, rd, rn, rm)
            }
            fn orr_x(&mut self, rd: u8, rn: u8, rm: u8) {
                <$ty>::orr_x(self, rd, rn, rm)
            }
            fn eor_w(&mut self, rd: u8, rn: u8, rm: u8) {
                <$ty>::eor_w(self, rd, rn, rm)
            }
            fn eor_x(&mut self, rd: u8, rn: u8, rm: u8) {
                <$ty>::eor_x(self, rd, rn, rm)
            }
            fn lslv_w(&mut self, rd: u8, rn: u8, rm: u8) {
                <$ty>::lslv_w(self, rd, rn, rm)
            }
            fn lslv_x(&mut self, rd: u8, rn: u8, rm: u8) {
                <$ty>::lslv_x(self, rd, rn, rm)
            }
            fn lsrv_w(&mut self, rd: u8, rn: u8, rm: u8) {
                <$ty>::lsrv_w(self, rd, rn, rm)
            }
            fn lsrv_x(&mut self, rd: u8, rn: u8, rm: u8) {
                <$ty>::lsrv_x(self, rd, rn, rm)
            }
            fn asrv_w(&mut self, rd: u8, rn: u8, rm: u8) {
                <$ty>::asrv_w(self, rd, rn, rm)
            }
            fn asrv_x(&mut self, rd: u8, rn: u8, rm: u8) {
                <$ty>::asrv_x(self, rd, rn, rm)
            }
            fn rorv_w(&mut self, rd: u8, rn: u8, rm: u8) {
                <$ty>::rorv_w(self, rd, rn, rm)
            }
            fn rorv_x(&mut self, rd: u8, rn: u8, rm: u8) {
                <$ty>::rorv_x(self, rd, rn, rm)
            }
            fn rbit_w(&mut self, rd: u8, rn: u8) {
                <$ty>::rbit_w(self, rd, rn)
            }
            fn rbit_x(&mut self, rd: u8, rn: u8) {
                <$ty>::rbit_x(self, rd, rn)
            }
            fn clz_w(&mut self, rd: u8, rn: u8) {
                <$ty>::clz_w(self, rd, rn)
            }
            fn clz_x(&mut self, rd: u8, rn: u8) {
                <$ty>::clz_x(self, rd, rn)
            }
            fn sxth_w(&mut self, rd: u8, rn: u8) {
                <$ty>::sxth_w(self, rd, rn)
            }
            fn sxtb_w(&mut self, rd: u8, rn: u8) {
                <$ty>::sxtb_w(self, rd, rn)
            }
            fn cmp_w(&mut self, rn: u8, rm: u8) {
                <$ty>::cmp_w(self, rn, rm)
            }
            fn cmp_x(&mut self, rn: u8, rm: u8) {
                <$ty>::cmp_x(self, rn, rm)
            }
            fn cmp_w_imm(&mut self, rn: u8, imm: u32) -> $crate::masm::AsmResult {
                <$ty>::cmp_w_imm(self, rn, imm)
            }
            fn cmp_w_u32(&mut self, rn: u8, imm: u32) {
                <$ty>::cmp_w_u32(self, rn, imm)
            }
            fn cset_w(&mut self, rd: u8, cond: $crate::masm::Cond) {
                <$ty>::cset_w(self, rd, cond)
            }
            fn csel_w(&mut self, rd: u8, rn: u8, rm: u8, cond: $crate::masm::Cond) {
                <$ty>::csel_w(self, rd, rn, rm, cond)
            }
            fn fmov_s_from_w(&mut self, vd: u8, rn: u8) {
                <$ty>::fmov_s_from_w(self, vd, rn)
            }
            fn fmov_w_from_s(&mut self, rd: u8, vn: u8) {
                <$ty>::fmov_w_from_s(self, rd, vn)
            }
            fn fmov_d_from_x(&mut self, vd: u8, rn: u8) {
                <$ty>::fmov_d_from_x(self, vd, rn)
            }
            fn fmov_x_from_d(&mut self, rd: u8, vn: u8) {
                <$ty>::fmov_x_from_d(self, rd, vn)
            }
            fn fcmp_s(&mut self, vn: u8, vm: u8) {
                <$ty>::fcmp_s(self, vn, vm)
            }
            fn fcmp_d(&mut self, vn: u8, vm: u8) {
                <$ty>::fcmp_d(self, vn, vm)
            }
            fn fadd_d(&mut self, vd: u8, vn: u8, vm: u8) {
                <$ty>::fadd_d(self, vd, vn, vm)
            }
            fn fadd_s(&mut self, vd: u8, vn: u8, vm: u8) {
                <$ty>::fadd_s(self, vd, vn, vm)
            }
            fn fsub_d(&mut self, vd: u8, vn: u8, vm: u8) {
                <$ty>::fsub_d(self, vd, vn, vm)
            }
            fn fsub_s(&mut self, vd: u8, vn: u8, vm: u8) {
                <$ty>::fsub_s(self, vd, vn, vm)
            }
            fn fmul_d(&mut self, vd: u8, vn: u8, vm: u8) {
                <$ty>::fmul_d(self, vd, vn, vm)
            }
            fn fmul_s(&mut self, vd: u8, vn: u8, vm: u8) {
                <$ty>::fmul_s(self, vd, vn, vm)
            }
            fn fdiv_d(&mut self, vd: u8, vn: u8, vm: u8) {
                <$ty>::fdiv_d(self, vd, vn, vm)
            }
            fn fdiv_s(&mut self, vd: u8, vn: u8, vm: u8) {
                <$ty>::fdiv_s(self, vd, vn, vm)
            }
            fn fabs_s(&mut self, vd: u8, vn: u8) {
                <$ty>::fabs_s(self, vd, vn)
            }
            fn fabs_d(&mut self, vd: u8, vn: u8) {
                <$ty>::fabs_d(self, vd, vn)
            }
            fn fneg_s(&mut self, vd: u8, vn: u8) {
                <$ty>::fneg_s(self, vd, vn)
            }
            fn fneg_d(&mut self, vd: u8, vn: u8) {
                <$ty>::fneg_d(self, vd, vn)
            }
            fn fsqrt_s(&mut self, vd: u8, vn: u8) {
                <$ty>::fsqrt_s(self, vd, vn)
            }
            fn fsqrt_d(&mut self, vd: u8, vn: u8) {
                <$ty>::fsqrt_d(self, vd, vn)
            }
            fn frintn_s(&mut self, vd: u8, vn: u8) -> $crate::masm::AsmResult {
                <$ty>::frintn_s(self, vd, vn)
            }
            fn frintn_d(&mut self, vd: u8, vn: u8) -> $crate::masm::AsmResult {
                <$ty>::frintn_d(self, vd, vn)
            }
            fn frintp_s(&mut self, vd: u8, vn: u8) -> $crate::masm::AsmResult {
                <$ty>::frintp_s(self, vd, vn)
            }
            fn frintp_d(&mut self, vd: u8, vn: u8) -> $crate::masm::AsmResult {
                <$ty>::frintp_d(self, vd, vn)
            }
            fn frintm_s(&mut self, vd: u8, vn: u8) -> $crate::masm::AsmResult {
                <$ty>::frintm_s(self, vd, vn)
            }
            fn frintm_d(&mut self, vd: u8, vn: u8) -> $crate::masm::AsmResult {
                <$ty>::frintm_d(self, vd, vn)
            }
            fn frintz_s(&mut self, vd: u8, vn: u8) -> $crate::masm::AsmResult {
                <$ty>::frintz_s(self, vd, vn)
            }
            fn frintz_d(&mut self, vd: u8, vn: u8) -> $crate::masm::AsmResult {
                <$ty>::frintz_d(self, vd, vn)
            }
            fn cvtf_d_from_w(&mut self, vd: u8, rn: u8, signed: bool) {
                <$ty>::cvtf_d_from_w(self, vd, rn, signed)
            }
            fn cvtf_s_from_w(&mut self, vd: u8, rn: u8, signed: bool) {
                <$ty>::cvtf_s_from_w(self, vd, rn, signed)
            }
            fn cvtf_d_from_x(&mut self, vd: u8, rn: u8, signed: bool) {
                <$ty>::cvtf_d_from_x(self, vd, rn, signed)
            }
            fn cvtf_s_from_x(&mut self, vd: u8, rn: u8, signed: bool) {
                <$ty>::cvtf_s_from_x(self, vd, rn, signed)
            }
            fn fcvt_s_from_d(&mut self, vd: u8, vn: u8) {
                <$ty>::fcvt_s_from_d(self, vd, vn)
            }
            fn fcvt_d_from_s(&mut self, vd: u8, vn: u8) {
                <$ty>::fcvt_d_from_s(self, vd, vn)
            }
            fn fcvt_w_from_s(&mut self, rd: u8, vn: u8, signed: bool) {
                <$ty>::fcvt_w_from_s(self, rd, vn, signed)
            }
            fn fcvt_w_from_d(&mut self, rd: u8, vn: u8, signed: bool) {
                <$ty>::fcvt_w_from_d(self, rd, vn, signed)
            }
            fn lsl_w_imm(&mut self, rd: u8, rn: u8, shift: u32) {
                <$ty>::lsl_w_imm(self, rd, rn, shift)
            }
            fn lsr_w_imm(&mut self, rd: u8, rn: u8, shift: u32) {
                <$ty>::lsr_w_imm(self, rd, rn, shift)
            }
            fn asr_w_imm(&mut self, rd: u8, rn: u8, shift: u32) {
                <$ty>::asr_w_imm(self, rd, rn, shift)
            }
            fn ubfm_w(&mut self, rd: u8, rn: u8, immr: u32, imms: u32) {
                <$ty>::ubfm_w(self, rd, rn, immr, imms)
            }
            fn ubfm_x(&mut self, rd: u8, rn: u8, immr: u32, imms: u32) {
                <$ty>::ubfm_x(self, rd, rn, immr, imms)
            }
            fn sbfm_w(&mut self, rd: u8, rn: u8, immr: u32, imms: u32) {
                <$ty>::sbfm_w(self, rd, rn, immr, imms)
            }
            fn sbfm_x(&mut self, rd: u8, rn: u8, immr: u32, imms: u32) {
                <$ty>::sbfm_x(self, rd, rn, immr, imms)
            }
            fn lsl_x_imm(&mut self, rd: u8, rn: u8, shift: u32) -> $crate::masm::AsmResult {
                <$ty>::lsl_x_imm(self, rd, rn, shift)
            }
            fn lsr_x_imm(&mut self, rd: u8, rn: u8, shift: u32) -> $crate::masm::AsmResult {
                <$ty>::lsr_x_imm(self, rd, rn, shift)
            }
            fn ldr_x_imm(&mut self, rt: u8, rn: u8, offset: usize) -> $crate::masm::AsmResult {
                <$ty>::ldr_x_imm(self, rt, rn, offset)
            }
            fn ldr_w_imm(&mut self, rt: u8, rn: u8, offset: usize) -> $crate::masm::AsmResult {
                <$ty>::ldr_w_imm(self, rt, rn, offset)
            }
            fn ldr_w_unscaled_imm(
                &mut self,
                rt: u8,
                rn: u8,
                offset: u32,
            ) -> $crate::masm::AsmResult {
                <$ty>::ldr_w_unscaled_imm(self, rt, rn, offset)
            }
            fn ldrb_w(&mut self, rt: u8, rn: u8) {
                <$ty>::ldrb_w(self, rt, rn)
            }
            fn ldrsb_w(&mut self, rt: u8, rn: u8) {
                <$ty>::ldrsb_w(self, rt, rn)
            }
            fn ldrh_w(&mut self, rt: u8, rn: u8) {
                <$ty>::ldrh_w(self, rt, rn)
            }
            fn ldrsh_w(&mut self, rt: u8, rn: u8) {
                <$ty>::ldrsh_w(self, rt, rn)
            }
            fn ldr_w(&mut self, rt: u8, rn: u8) {
                <$ty>::ldr_w(self, rt, rn)
            }
            fn strb_w(&mut self, rt: u8, rn: u8) {
                <$ty>::strb_w(self, rt, rn)
            }
            fn strh_w(&mut self, rt: u8, rn: u8) {
                <$ty>::strh_w(self, rt, rn)
            }
            fn str_w(&mut self, rt: u8, rn: u8) {
                <$ty>::str_w(self, rt, rn)
            }
            fn str_x_imm(&mut self, rt: u8, rn: u8, offset: usize) -> $crate::masm::AsmResult {
                <$ty>::str_x_imm(self, rt, rn, offset)
            }
            fn str_w_imm(&mut self, rt: u8, rn: u8, offset: usize) -> $crate::masm::AsmResult {
                <$ty>::str_w_imm(self, rt, rn, offset)
            }
            fn str_w_unscaled_imm(
                &mut self,
                rt: u8,
                rn: u8,
                offset: u32,
            ) -> $crate::masm::AsmResult {
                <$ty>::str_w_unscaled_imm(self, rt, rn, offset)
            }
            fn patch_branch(
                bytes: &mut [u8],
                at: usize,
                target: usize,
                kind: $crate::masm::BranchKind,
            ) -> $crate::masm::AsmResult {
                patch_branch(bytes, at, target, kind)
            }
        }
    };
}
