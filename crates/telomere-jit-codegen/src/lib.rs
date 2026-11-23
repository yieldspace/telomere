//! Executable memory and code emission helpers for the telomere baseline JIT.
//!
//! This crate provides the low-level pieces the core JIT builds on: mapping and
//! protecting executable code memory, a small macro assembler, and the
//! per-architecture emitters for the supported native targets. It has no
//! knowledge of WebAssembly semantics; the core crate drives it.
//!
//! The macro assemblers (`arch`, `masm`) sit behind the `emit` feature: they
//! are only needed to *produce* native code. A deployment that merely loads
//! and executes precompiled code (the core crate's `aot-load` feature) links
//! `code_memory` and `target` without them.

#[cfg(feature = "emit")]
pub mod arch;
pub mod code_memory;
#[cfg(feature = "emit")]
pub mod masm;
pub mod target;
