mod impls;
mod instance_ext;
#[cfg(test)]
mod tests;
mod traits;
mod typecheck;
mod typed;

use crate::ir::types::{DefValType, PrimValType, Type, ValType};
use crate::runtime::RuntimeInstance;
use crate::support::Store;
use crate::{ComponentError, ComponentInstance, ComponentProgram, ComponentValue};
use std::marker::PhantomData;
use std::rc::Rc;

pub use traits::{
    Borrow, ComponentErrorContext, ComponentFutureHandle, ComponentParams, ComponentReturn,
    ComponentStreamHandle, LiftComponent, LowerComponent, Own,
};
pub use typed::{ComponentFunc, TypedComponentFunc};
