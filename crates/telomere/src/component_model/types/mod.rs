mod alias;
mod canon;
mod component;
pub mod flatten;
mod func;
mod instance;
mod prim;
mod resource;
mod val;

use crate::parser::component_model::ComponentParseError;
pub use alias::*;
pub use canon::*;
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

#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    DefVal(Box<DefValType>),
    Func(FuncType),
    Component(ComponentType),
    Instance(InstanceType),
    Resource(ResourceType),
}

impl Type {
    pub fn is_resource_type(&self) -> bool {
        matches!(self, Type::Resource(_))
    }
}

impl_try_into_type!(FuncType, Func);
impl_try_into_type!(ComponentType, Component);
impl_try_into_type!(InstanceType, Instance);

impl TryInto<ResourceType> for Type {
    type Error = ComponentParseError;

    fn try_into(self) -> Result<ResourceType, Self::Error> {
        match self {
            Type::Resource(ty) => Ok(ty),
            _ => Err(ComponentParseError::InvalidType("Resource".to_string())),
        }
    }
}

impl TryFrom<ExternDesc> for Type {
    type Error = ComponentParseError;

    fn try_from(value: ExternDesc) -> Result<Self, Self::Error> {
        if let ExternDesc::Type(ty) = value {
            Ok(ty)
        } else {
            Err(ComponentParseError::InvalidType("Type".to_string()))
        }
    }
}
