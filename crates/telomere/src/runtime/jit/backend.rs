#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod aarch64_macos;
mod ops;
#[cfg(all(target_os = "linux", target_arch = "riscv64", target_env = "gnu"))]
mod riscv64;
#[cfg(all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"))]
mod x86_64;

use super::emit_env::{AotEmitEnv, Emission, EmitEnv, JitEmitEnv, ModuleFacts, Reloc, RelocKind};

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
    let env = JitEmitEnv::new(runtime);
    let emission = emit_baseline_plan(funcaddr, code, op_lens, env, &plan)
        .map_err(|_| EmitBaselineError::Emit)?;
    debug_assert!(
        emission.relocs.is_empty(),
        "JIT-mode emission must not record relocations"
    );
    Ok(EmittedFunction {
        code: emission.bytes,
        trap_sites: emission.trap_sites,
    })
}

/// Emits one function in AoT mode (#197): the returned bytes hold
/// placeholder immediates at every relocation site, and the relocation
/// table in the [`Emission`] describes what each site must be patched
/// with once the module is instantiated.
// #197: exercised by the relocation differential tests; the production
// consumer is the AoT loader from the container work.
#[allow(dead_code)]
pub(crate) fn emit_baseline_function_aot(
    code: &[crate::common::Instr],
    op_lens: &[u16],
    facts: ModuleFacts<'_>,
) -> Result<Emission, EmitBaselineError> {
    let plan = ops::build_baseline_plan(code, op_lens).map_err(|_| EmitBaselineError::Verify)?;
    let env = AotEmitEnv::new(facts);
    // AoT emission has no current function in any store; the null
    // ObjectRef is never read by the AoT environment.
    emit_baseline_plan(crate::common::ObjectRef(0), code, op_lens, env, &plan)
        .map_err(|_| EmitBaselineError::Emit)
}

/// Patches every relocation site in `bytes` with the value `resolve`
/// returns for its kind. Each site is verified against the placeholder
/// encoding for its kind before anything is written (#197 patch-site
/// verification), so a relocation recorded against different bytes fails
/// instead of corrupting code.
// #197: exercised by the relocation differential tests; the production
// consumer is the AoT loader from the container work.
#[allow(dead_code)]
pub(crate) fn patch_relocations(
    bytes: &mut [u8],
    relocs: &[Reloc],
    resolve: &dyn Fn(RelocKind) -> u64,
) -> Result<(), EmitBaselineError> {
    for reloc in relocs {
        let value = resolve(reloc.kind);
        patch_relocation_site(bytes, *reloc, value).map_err(|_| EmitBaselineError::Emit)?;
    }
    Ok(())
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn emit_baseline_plan<E: EmitEnv>(
    funcaddr: crate::common::ObjectRef,
    code: &[crate::common::Instr],
    op_lens: &[u16],
    env: E,
    plan: &ops::BaselinePlan,
) -> Result<Emission, ()> {
    aarch64_macos::emit_baseline_function(funcaddr, code, op_lens, env, plan)
}

#[cfg(all(target_os = "linux", target_arch = "riscv64", target_env = "gnu"))]
fn emit_baseline_plan<E: EmitEnv>(
    funcaddr: crate::common::ObjectRef,
    code: &[crate::common::Instr],
    op_lens: &[u16],
    env: E,
    plan: &ops::BaselinePlan,
) -> Result<Emission, ()> {
    riscv64::emit_baseline_function(funcaddr, code, op_lens, env, plan)
}

#[cfg(all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"))]
fn emit_baseline_plan<E: EmitEnv>(
    funcaddr: crate::common::ObjectRef,
    code: &[crate::common::Instr],
    op_lens: &[u16],
    env: E,
    plan: &ops::BaselinePlan,
) -> Result<Emission, ()> {
    x86_64::emit_baseline_function(funcaddr, code, op_lens, env, plan)
}

#[cfg(not(any(
    all(target_os = "macos", target_arch = "aarch64"),
    all(target_os = "linux", target_arch = "riscv64", target_env = "gnu"),
    all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64")
)))]
fn emit_baseline_plan<E: EmitEnv>(
    _funcaddr: crate::common::ObjectRef,
    _code: &[crate::common::Instr],
    _op_lens: &[u16],
    _env: E,
    _plan: &ops::BaselinePlan,
) -> Result<Emission, ()> {
    Err(())
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[allow(dead_code)]
fn patch_relocation_site(bytes: &mut [u8], reloc: Reloc, value: u64) -> Result<(), ()> {
    aarch64_macos::patch_relocation_site(bytes, reloc, value)
}

#[cfg(all(target_os = "linux", target_arch = "riscv64", target_env = "gnu"))]
#[allow(dead_code)]
fn patch_relocation_site(bytes: &mut [u8], reloc: Reloc, value: u64) -> Result<(), ()> {
    riscv64::patch_relocation_site(bytes, reloc, value)
}

#[cfg(all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"))]
#[allow(dead_code)]
fn patch_relocation_site(bytes: &mut [u8], reloc: Reloc, value: u64) -> Result<(), ()> {
    x86_64::patch_relocation_site(bytes, reloc, value)
}

#[cfg(not(any(
    all(target_os = "macos", target_arch = "aarch64"),
    all(target_os = "linux", target_arch = "riscv64", target_env = "gnu"),
    all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64")
)))]
#[allow(dead_code)]
fn patch_relocation_site(_bytes: &mut [u8], _reloc: Reloc, _value: u64) -> Result<(), ()> {
    Err(())
}
