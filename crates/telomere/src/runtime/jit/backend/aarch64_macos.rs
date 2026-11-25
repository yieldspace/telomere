//! aarch64 (macOS) target hooks for the shared baseline lowering.
//!
//! aarch64 uses every [`TargetHooks`] default (native `frint`
//! rounding, native `fcvt` trunc-sat, plain `sdiv` + `msub`
//! remainder), so the impl is empty. The target-specific code for
//! this platform is the encoder in
//! `telomere_jit_codegen::arch::aarch64`.

use telomere_jit_codegen::arch::aarch64::A64BaselineMasm;

use super::lowering::TargetHooks;

impl TargetHooks for A64BaselineMasm {}
