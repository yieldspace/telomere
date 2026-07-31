//! WebAssembly Component Model decoder, IR, linker, and runtime for telomere.
//!
//! This crate turns a component binary into a [`ComponentProgram`] through
//! [`ComponentEngine::compile`], resolves its imports against a
//! [`ComponentLinker`], and executes its exports over the core runtime in
//! `telomere`. It implements the canonical ABI, the component type system, and
//! resource handling itself; component execution is interpreted, with core
//! module bodies delegated to the core runtime rather than JIT-compiled at the
//! component level.

pub mod decoder;
mod engine;
mod error;
mod func;
mod instance;
pub mod ir;
mod linker;
mod program;
pub mod runtime;
mod support;
pub mod validate;
mod value;

use std::future::Future;
use std::pin::Pin;

pub use engine::ComponentEngine;
pub use error::ComponentError;
pub use func::{Borrow, ComponentFunc, LiftComponent, LowerComponent, Own, TypedComponentFunc};
#[doc(hidden)]
pub use func::{ComponentParams, ComponentReturn};
pub use instance::{ComponentExports, ComponentInstance};
pub use linker::{ComponentLinker, ComponentLinkerInstance};
pub use program::{ComponentOp, ComponentProgram, ComponentTypeInfo};
pub use support::Store;
pub use value::ComponentValue;

pub type ComponentFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;
