use crate::component_model::types::{PrimValType, TypeId};
use crate::component_model::Label;
use crate::component_model::types::placeholder::{ResolveContext, TypeKind};
use crate::parser::component_model::ParseResult;

#[derive(Debug, Clone, PartialEq, Hash, Eq)]
pub enum ValType {
    Type(TypeId),
    Primitive(PrimValType),
}

impl TypeKind for ValType {
    fn resolve(&mut self, ctx: &mut ResolveContext) -> ParseResult<()> {
        todo!()
    }

    fn is_eq_or_super_type_of(&self, other: &Self) -> bool {
        todo!()
    }
}

#[derive(Debug, Clone, PartialEq, Hash, Eq)]
pub struct LabelValType {
    pub label: Label,
    pub ty: ValType,
}
