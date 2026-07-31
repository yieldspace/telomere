//! Executable memory and code emission helpers for the telomere baseline JIT.
//!
//! This crate provides the low-level pieces the core JIT builds on: mapping and
//! protecting executable code memory, a small macro assembler, and the
//! per-architecture emitters for the supported native targets. It has no
//! knowledge of WebAssembly semantics; the core crate drives it.

pub mod arch;
pub mod code_memory;
pub mod masm;
pub mod target;
