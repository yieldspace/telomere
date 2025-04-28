mod alias;
mod component;
mod func;
mod instance;
mod prim;
mod resource;
mod val;
mod idx;

use crate::component_model::{Reference, TypeIdx};
use crate::parser::component_model::ComponentParseError;
pub use alias::*;
pub use component::*;
pub use func::*;
pub use instance::*;
pub use prim::*;
pub use resource::*;
pub use val::*;

macro_rules! impl_try_into_type {
    ($from:ident, $variant:ident) => {
        impl TryFrom<Type> for $from {
            type Error = ComponentParseError;
            fn try_from(value: Type) -> Result<Self, Self::Error> {
                if let Type::$variant(value) = value {
                    Ok(value)
                } else {
                    Err(ComponentParseError::InvalidType(
                        stringify!($variant).to_string(),
                    ))
                }
            }
        }
    };
}

#[derive(Debug, Clone)]
pub enum Type {
    DefVal(Box<DefValType>),
    Func(FuncType),
    Component(ComponentType),
    Instance(InstanceType),
    Resource(ResourceType),
    // from (sub resource)
    // todo: 処理系はatomic usize等でunique性を担保する
    UniqueResource(usize), // (usize)
    Eq(TypeIdx),
    Referenced(Box<Type>, Reference),
}

impl_try_into_type!(FuncType, Func);
impl_try_into_type!(ComponentType, Component);
impl_try_into_type!(InstanceType, Instance);
impl_try_into_type!(ResourceType, Resource);

impl Type {
    /// Checks if the type is a resource type.
    pub fn is_resource_type(&self) -> bool {
        todo!()
    }
}
