#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod aarch64_macos;
mod ops;
#[cfg(all(target_os = "linux", target_arch = "riscv64", target_env = "gnu"))]
mod riscv64;
#[cfg(all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"))]
mod x86_64;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub(crate) use aarch64_macos::emit_baseline_function;
#[cfg(all(target_os = "linux", target_arch = "riscv64", target_env = "gnu"))]
pub(crate) use riscv64::emit_baseline_function;
#[cfg(all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"))]
pub(crate) use x86_64::emit_baseline_function;

#[cfg(not(any(
    all(target_os = "macos", target_arch = "aarch64"),
    all(target_os = "linux", target_arch = "riscv64", target_env = "gnu"),
    all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64")
)))]
pub(crate) fn emit_baseline_function(
    _funcaddr: crate::common::ObjectRef,
    _code: &[crate::common::Instr],
    _op_lens: &[u16],
    _gc: &crate::common::store::StoreInner,
) -> Result<Vec<u8>, ()> {
    Err(())
}

pub(crate) fn supported() -> bool {
    telomere_jit_codegen::target::supported()
}
