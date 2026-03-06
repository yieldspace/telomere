pub mod decoder;
mod engine;
mod error;
mod instance;
pub mod ir;
mod linker;
mod program;
pub mod runtime;
pub mod validate;
mod value;

use std::future::Future;
use std::pin::Pin;

pub use engine::ComponentEngine;
pub use error::ComponentError;
pub use instance::ComponentInstance;
pub use linker::ComponentLinker;
pub use program::{ComponentOp, ComponentProgram, ComponentTypeInfo};
pub use value::ComponentValue;

pub type ComponentFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;
