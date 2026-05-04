#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod aarch64_macos;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub(crate) use aarch64_macos::emit_baseline_function;

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
pub(crate) fn emit_baseline_function(
    _funcaddr: crate::common::ObjectRef,
    _code: &[crate::common::Instr],
    _op_lens: &[u16],
) -> Result<Vec<u8>, ()> {
    Err(())
}

pub(crate) fn supported() -> bool {
    cfg!(all(target_os = "macos", target_arch = "aarch64"))
}
