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
pub use func::{ComponentFunc, LiftComponent, LowerComponent, TypedComponentFunc};
#[doc(hidden)]
pub use func::{ComponentParams, ComponentReturn};
pub use instance::{ComponentExports, ComponentInstance};
pub use linker::{ComponentLinker, ComponentLinkerInstance};
pub use program::{ComponentOp, ComponentProgram, ComponentTypeInfo};
pub use support::Store;
pub use value::ComponentValue;

pub type ComponentFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;
