use crate::component_model::types::{Type, TypeId};
use crate::component_model::PlaceholderId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TyRef<T = Type> {
    Defer(PlaceholderId),
    Const(T),
}

impl<T> TyRef<T> {
    pub fn new(ty: T) -> Self {
        Self::Const(ty)
    }

    pub fn defer(id: PlaceholderId, ty: T) -> Self {
        Self::Defer(id)
    }

    pub fn is_deferred(&self) -> bool {
        matches!(self, Self::Defer(_))
    }

    // /// deferであっても型を取得します
    // pub fn get_unresolved(&self) -> &T {
    //     match self {
    //         Self::Defer(_, id) => id,
    //         Self::Const(ty) => ty,
    //     }
    // }
}
