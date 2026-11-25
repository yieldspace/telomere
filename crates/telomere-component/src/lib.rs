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

/// Component bundles: hash-keyed AoT artifacts for the core modules
/// embedded in a component (#201).
#[cfg(feature = "aot-bundle")]
#[warn(missing_docs)]
pub mod bundle;
// The decoder is shared by the compiler and runtime, but is not an embedder API.
pub(crate) mod decoder;
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
// The component IR is an implementation detail shared within this crate.
pub(crate) mod ir;
/// Import and export registrations used during instantiation.
#[warn(missing_docs)]
mod linker;
/// The compiled component representation and canonical ABI metadata.
#[warn(missing_docs)]
mod program;
// The component runtime is internal to compilation and instantiation.
pub(crate) mod runtime;
mod support;
// This intentional crate-internal alias exposes decoder validation types without
// making the decoder implementation an embedder API.
pub(crate) mod validate;
/// Values passed across a component boundary.
#[warn(missing_docs)]
mod value;

#[doc(hidden)]
#[path = "bindgen_support.rs"]
pub mod __bindgen;

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

#[cfg(feature = "aot-bundle")]
#[doc(inline)]
pub use bundle::{
    build_component_bundle, is_component_bundle, BundleError, BundleLoadReport,
    ComponentBundleArtifacts, LoadedComponentBundle,
};
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
///
/// # First-poll contract for host functions
///
/// The component runtime executes host functions **synchronously**. When a
/// guest call reaches a host function registered with one of the `*_async`
/// linker methods, the returned future is polled exactly once, with a no-op
/// waker, on the thread running the guest. If that poll returns
/// `Poll::Pending`, the future is dropped and the call fails: it is never
/// polled again, and no executor will ever drive it to completion. Awaiting
/// futures that are already ready (such as `std::future::ready`) is fine;
/// awaiting anything that can genuinely suspend is not.
///
/// This is a deliberate, recorded decision (issue #147), not an oversight:
/// the synchronous registration forms are the supported way to provide host
/// functions today, and the `*_async` forms exist for futures that are
/// ready on their first poll. What genuine asynchronous host execution
/// would require is recorded in
/// `docs/component-model/new-component-runtime.md`.
pub type ComponentFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;
