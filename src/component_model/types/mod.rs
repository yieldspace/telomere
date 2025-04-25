mod prim;
mod func;
mod val;
mod resource;
mod component;
mod instance;
mod alias;

pub use prim::*;
pub use func::*;
pub use val::*;
pub use resource::*;
pub use instance::*;
pub use component::*;
pub use alias::*;
use crate::component_model::{Reference, TypeIdx};
use crate::parser::component_model::ComponentParseError;

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
    DefVal(DefValType),
    Func(FuncType),
    Component(ComponentType),
    Instance(InstanceType),
    Resource(ResourceType),
    // from (sub resource)
    // todo: 処理系はatomic usize等でunique性を担保する
    UniqueResource, // (usize)
    Eq(TypeIdx),
    SuperTypedUniqueResource(TypeIdx),
    Referenced(Box<Type>, Reference),
}

impl_try_into_type!(FuncType, Func);
impl_try_into_type!(ComponentType, Component);
impl_try_into_type!(InstanceType, Instance);
