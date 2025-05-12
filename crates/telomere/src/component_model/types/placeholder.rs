use std::collections::HashMap;
use crate::component_model::PlaceholderId;
use crate::component_model::types::TypeId;
use crate::parser::component_model::{ParseResult, ScopeGuard};

pub trait TypeKind {
    fn resolve(&mut self, ctx: &mut ResolveContext) -> ParseResult<()>;
    /// check if the self is a super type of other
    fn is_eq_or_super_type_of(&self, other: &Self) -> bool;
}

pub struct ResolveContext<'a, 'b> {
    pub scope: &'b mut ScopeGuard<'a>,
    pub placeholders: HashMap<PlaceholderId, TypeId>,
}

impl<'a, 'b> ResolveContext<'a, 'b> {
    pub fn new(scope: &'b mut ScopeGuard<'a>, placeholders: HashMap<PlaceholderId, TypeId>) -> Self {
        Self {
            scope,
            placeholders,
        }
    }

    pub fn get_new_type(&self, id: &PlaceholderId) -> Option<TypeId> {
        self.placeholders.get(id).cloned()
    }
}
