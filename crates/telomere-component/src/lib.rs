//! WebAssembly Component Model decoder, IR, linker, and runtime for telomere.
//!
//! This crate turns a component binary into a [`ComponentProgram`] through
//! [`ComponentEngine::compile`], resolves its imports against a
//! [`ComponentLinker`], and executes its exports over the core runtime in
//! `telomere`. It implements the canonical ABI, the component type system, and
//! resource handling itself; component execution is interpreted, with core
//! module bodies delegated to the core runtime rather than JIT-compiled at the
//! component level.

#![warn(missing_docs)]

// The decoder is public for advanced inspection, but is not the embedder API.
// TODO(#142-followup): document its stable, supported subset before lifting this allow.
#[allow(missing_docs)]
pub mod decoder;
/// The high-level component compiler and instantiator.
#[warn(missing_docs)]
mod engine;
/// Errors returned by component compilation, linking, and execution.
#[warn(missing_docs)]
mod error;
/// Typed and dynamically typed component-function helpers.
#[warn(missing_docs)]
mod func;
/// Component instances and their nested exports.
#[warn(missing_docs)]
mod instance;
// The IR is exposed for advanced tooling, not as a documented embedder surface.
// TODO(#142-followup): document the supported IR contracts before lifting this allow.
#[allow(missing_docs)]
pub mod ir;
/// Import and export registrations used during instantiation.
#[warn(missing_docs)]
mod linker;
/// The compiled component representation and canonical ABI metadata.
#[warn(missing_docs)]
mod program;
// Component runtime internals are public for tooling but not yet a supported API.
// TODO(#142-followup): document the supported runtime hooks before lifting this allow.
#[allow(missing_docs)]
pub mod runtime;
mod support;
// Validation internals are public for advanced tooling, not the embedder API.
// TODO(#142-followup): document the supported validation entry points before lifting this allow.
#[allow(missing_docs)]
pub mod validate;
/// Values passed across a component boundary.
#[warn(missing_docs)]
mod value;

/// Shared maximum nesting depth of 100 for component sections and component/instance type declarations.
///
/// This limit is part of the optimized-build input-decoder stack-budget policy. The root decode
/// starts at `depth = 0`, and the decoder rejects `depth > limit`, so 100 nested constructs are
/// accepted and 101 are rejected with [`ComponentError::NestingTooDeep`].
///
/// See the [parser limits guide](https://github.com/yieldspace/telomere/blob/main/docs/core/parser-limits.md)
/// for the policy scope and measurement boundary; it does not claim a stack bound for all of
/// [`ComponentEngine::compile`].
pub const MAX_COMPONENT_NESTING_DEPTH: u32 = 100;

use std::future::Future;
use std::pin::Pin;

#[doc(inline)]
pub use engine::ComponentEngine;
#[doc(inline)]
pub use error::ComponentError;
#[doc(inline)]
pub use func::{Borrow, ComponentFunc, LiftComponent, LowerComponent, Own, TypedComponentFunc};
#[doc(hidden)]
pub use func::{ComponentParams, ComponentReturn};
#[doc(inline)]
pub use instance::{ComponentExports, ComponentInstance};
#[doc(inline)]
pub use linker::{ComponentLinker, ComponentLinkerInstance};
#[doc(inline)]
pub use program::{ComponentOp, ComponentProgram, ComponentTypeInfo};
#[cfg(feature = "fuzzing")]
#[doc(hidden)]
pub use runtime::{
    fuzz_canonical_lift_args, fuzz_canonical_lift_args_from_bytes, FuzzCanonicalLiftInput,
    FuzzStringEncoding, MAX_FUZZ_MEMORY_IMAGE_BYTES,
};
/// The core-runtime store used to allocate and execute a component instance.
///
/// This is a re-export of [`telomere::Store`], so one store can own both core
/// modules and components that depend on them.
#[doc(inline)]
pub use support::Store;
#[doc(inline)]
pub use value::ComponentValue;

/// A boxed, non-`Send` future used by asynchronous component APIs and host functions.
///
/// Component execution currently uses `Rc`-backed state, so callers should poll
/// this future on the thread that owns the associated [`Store`].
pub type ComponentFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;
