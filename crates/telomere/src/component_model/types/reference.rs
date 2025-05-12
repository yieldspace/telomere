use crate::component_model::types::{Type, TypeId};
use crate::component_model::{PlaceholderId, ResourceId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TyRef<T = Type> {
    DeferType(PlaceholderId, ResourceId),
    DeferResource(PlaceholderId, TypeId),
    Const(T),
}

impl<T> TyRef<T> {
    pub fn new(ty: T) -> Self {
        Self::Const(ty)
    }

    pub fn defer(id: PlaceholderId, ty: TypeId) -> Self {
        Self::Defer(id, ty)
    }

    pub fn is_deferred(&self) -> bool {
        matches!(self, Self::Defer(_, _))
    }

    // /// deferであっても型を取得します
    // pub fn get_unresolved(&self) -> &T {
    //     match self {
    //         Self::Defer(_, id) => id,
    //         Self::Const(ty) => ty,
    //     }
    // }
}
