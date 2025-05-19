use crate::component_model::types::{ComponentType, DefValType, InstanceType};
use crate::component_model::ResourceId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    DefVal(DefValType),
    Component(ComponentType),
    Instance(InstanceType),
    Resource(ResourceId),
}

impl Type {
    pub fn is_component_type(&self) -> bool {
        matches!(self, Self::Component(_))
    }

    pub fn is_function_type(&self) -> bool {
        // todo
        false
    }
}

macro_rules! impl_try_from {
    ($ty:ident, $variant:ident) => {
        impl TryFrom<Type> for $ty {
            type Error = String;

            fn try_from(value: Type) -> Result<Self, Self::Error> {
                if let Type::$variant(inner) = value {
                    Ok(inner)
                } else {
                    Err("wrong type".to_string())
                }
            }
        }
    };
}

impl_try_from!(ComponentType, Component);
impl_try_from!(InstanceType, Instance);
impl_try_from!(ResourceId, Resource);
