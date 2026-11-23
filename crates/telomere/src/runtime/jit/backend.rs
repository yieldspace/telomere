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

/// One compiled function: the native code plus its #209 trap-site table.
///
/// `trap_sites[site - 1]` is the decoded-stream pc index of the
/// instruction whose emission produced trap site `site` (site ids are
/// 1-based; site 0 in a trap exit means the exit carries no site). The
/// table is a plain list of pc indices so it stays trivially
/// serializable for the AoT path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EmittedFunction {
    pub(crate) code: Vec<u8>,
    pub(crate) trap_sites: Vec<u32>,
}

pub(crate) fn emit_baseline_function(
    funcaddr: crate::common::ObjectRef,
    code: &[crate::common::Instr],
    op_lens: &[u16],
    runtime: &crate::common::store::StoreInner,
) -> Result<EmittedFunction, EmitBaselineError> {
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
) -> Result<EmittedFunction, ()> {
    aarch64_macos::emit_baseline_function(funcaddr, code, op_lens, runtime, plan)
}

#[cfg(all(target_os = "linux", target_arch = "riscv64", target_env = "gnu"))]
fn emit_baseline_plan(
    funcaddr: crate::common::ObjectRef,
    code: &[crate::common::Instr],
    op_lens: &[u16],
    runtime: &crate::common::store::StoreInner,
    plan: &ops::BaselinePlan,
) -> Result<EmittedFunction, ()> {
    riscv64::emit_baseline_function(funcaddr, code, op_lens, runtime, plan)
}

#[cfg(all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"))]
fn emit_baseline_plan(
    funcaddr: crate::common::ObjectRef,
    code: &[crate::common::Instr],
    op_lens: &[u16],
    runtime: &crate::common::store::StoreInner,
    plan: &ops::BaselinePlan,
) -> Result<EmittedFunction, ()> {
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
) -> Result<EmittedFunction, ()> {
    Err(())
}

pub(crate) fn supported() -> bool {
    telomere_jit_codegen::target::supported()
}
