use std::collections::HashMap;
use crate::component_model::types::{ComponentType, DefValType, InstanceType};
use crate::component_model::{PlaceholderId, ResourceId};
use crate::component_model::types::placeholder::{ResolveContext, TypeKind};
use crate::parser::component_model::ParseResult;

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
    
    pub fn is_instance_type(&self) -> bool {
        matches!(self, Self::Instance(_))
    }

    pub fn is_function_type(&self) -> bool {
        // todo
        false
    }
}

impl TypeKind for Type {
    fn resolve(&mut self, ctx: &mut ResolveContext) -> ParseResult<()> {
        match self {
            Type::DefVal(ty) => ty.resolve(ctx),
            Type::Component(ty) => ty.resolve(ctx),
            Type::Instance(ty) => ty.resolve(ctx),
            Type::Resource(_) => Ok(()),
        }
    }

    fn is_eq_or_super_type_of(&self, other: &Self) -> bool {
        todo!()
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
