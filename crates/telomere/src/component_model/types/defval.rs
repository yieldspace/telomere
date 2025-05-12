use crate::component_model::types::{LabelValType, PrimValType, TyRef, TypeId, ValType};
use crate::component_model::{Label, ResourceId};
use crate::component_model::types::placeholder::{ResolveContext, TypeKind};
use crate::parser::component_model::ParseResult;

#[derive(Debug, Clone, PartialEq, Hash, Eq)]
pub enum DefValType {
    Primitive(PrimValType),
    Record(Vec<LabelValType>),
    Variant(Vec<Case>),
    List(ValType, Option<usize>),
    Tuple(Vec<ValType>),
    Option(ValType),
    Result(Option<ValType>, Option<ValType>),
    Own(TypeId),
    Borrow(TypeId),
}

#[derive(Debug, Clone, PartialEq, Hash, Eq)]
pub struct Case {
    label: Label,
    ty: ValType,
}

impl TypeKind for DefValType {
    fn resolve(&mut self, ctx: &mut ResolveContext) -> ParseResult<()> {
        match self {
            DefValType::Primitive(_) => {},
            DefValType::Record(labels) => {
                for label in labels.iter_mut() {
                    label.ty.resolve(ctx)?;
                }
            }
            DefValType::Variant(ty) => {
                for case in ty.iter_mut() {
                    case.ty.resolve(ctx)?;
                }
            }
            DefValType::List(ty, _) => {
                ty.resolve(ctx)?;
            }
            DefValType::Tuple(ty) => {
                for ty in ty.iter_mut() {
                    ty.resolve(ctx)?;
                }
            }
            DefValType::Option(ty) => {
                ty.resolve(ctx)?;
            }
            DefValType::Result(k, v) => {
                if let Some(ty) = k {
                    ty.resolve(ctx)?;
                }
                if let Some(ty) = v {
                    ty.resolve(ctx)?;
                }
            }
            DefValType::Own(id) => {
                let ty = ctx.scope.get_tyref(*id)?.clone();
                if let TyRef::Defer(pid, _) = ty {
                    if let Some(new_ty) = ctx.get_new_type(&pid) {
                        *self = DefValType::Own(new_ty);
                    }
                }
            }
            DefValType::Borrow(id) => {
                let ty = ctx.scope.get_tyref(*id)?.clone();
                if let TyRef::Defer(pid, _) = ty {
                    if let Some(new_ty) = ctx.get_new_type(&pid) {
                        *self = DefValType::Borrow(new_ty);
                    }
                }
            }
        }
        Ok(())
    }

    fn is_eq_or_super_type_of(&self, other: &Self) -> bool {
        todo!()
    }
}
