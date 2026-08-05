#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod aarch64_macos;
mod ops;
#[cfg(all(target_os = "linux", target_arch = "riscv64", target_env = "gnu"))]
mod riscv64;
#[cfg(all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"))]
mod x86_64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EmitBaselineError {
    Verify,
    Emit,
}

pub(crate) struct BaselineArtifact {
    pub(crate) code: Vec<u8>,
    pub(crate) trap_sites: Vec<u32>,
}

pub(crate) fn emit_baseline_function(
    funcaddr: crate::common::ObjectRef,
    code: &[crate::common::Instr],
    op_lens: &[u16],
    runtime: &crate::common::store::StoreInner,
) -> Result<BaselineArtifact, EmitBaselineError> {
    let plan = ops::build_baseline_plan(code, op_lens).map_err(|_| EmitBaselineError::Verify)?;
    emit_baseline_plan(funcaddr, code, op_lens, runtime, &plan).map_err(|_| EmitBaselineError::Emit)
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn emit_baseline_plan(
    funcaddr: crate::common::ObjectRef,
    code: &[crate::common::Instr],
    op_lens: &[u16],
    runtime: &crate::common::store::StoreInner,
    plan: &ops::BaselinePlan,
) -> Result<BaselineArtifact, ()> {
    aarch64_macos::emit_baseline_function(funcaddr, code, op_lens, runtime, plan)
}

#[cfg(all(target_os = "linux", target_arch = "riscv64", target_env = "gnu"))]
fn emit_baseline_plan(
    funcaddr: crate::common::ObjectRef,
    code: &[crate::common::Instr],
    op_lens: &[u16],
    runtime: &crate::common::store::StoreInner,
    plan: &ops::BaselinePlan,
) -> Result<BaselineArtifact, ()> {
    riscv64::emit_baseline_function(funcaddr, code, op_lens, runtime, plan)
}

#[cfg(all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"))]
fn emit_baseline_plan(
    funcaddr: crate::common::ObjectRef,
    code: &[crate::common::Instr],
    op_lens: &[u16],
    runtime: &crate::common::store::StoreInner,
    plan: &ops::BaselinePlan,
) -> Result<BaselineArtifact, ()> {
    x86_64::emit_baseline_function(funcaddr, code, op_lens, runtime, plan)
}

#[cfg(not(any(
    all(target_os = "macos", target_arch = "aarch64"),
    all(target_os = "linux", target_arch = "riscv64", target_env = "gnu"),
    all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64")
)))]
fn emit_baseline_plan(
    _funcaddr: crate::common::ObjectRef,
    _code: &[crate::common::Instr],
    _op_lens: &[u16],
    _runtime: &crate::common::store::StoreInner,
    _plan: &ops::BaselinePlan,
) -> Result<BaselineArtifact, ()> {
    Err(())
}

pub(crate) fn supported() -> bool {
    telomere_jit_codegen::target::supported()
}
