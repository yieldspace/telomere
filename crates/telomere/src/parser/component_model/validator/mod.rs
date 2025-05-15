mod scope;
mod state;

use crate::component_model::types::Type;
use crate::component_model::{Component, GlobalIdx, Instance, LocalIdx, TypeId};
use crate::parser::component_model::ParseResult;
pub use scope::ScopeGuard;
pub use state::ParseState;
use std::cell::{RefCell, RefMut};
use std::collections::HashMap;
use tracing::trace;
use typed_arena::Arena;

pub struct Validator<'a> {
    arena: &'a Arena<ScopeGuard>,
    scopes: Vec<&'a mut ScopeGuard>,
    types: HashMap<TypeId, Type>,
}

impl<'a> Validator<'a> {
    pub fn new(arena: &'a Arena<ScopeGuard>) -> Self {
        let current = arena.alloc(ScopeGuard::new());
        Self {
            arena,
            scopes: vec![current],
            types: HashMap::new(),
        }
    }

    pub fn push_scope(&mut self) {
        trace!("Validator::push_scope");
        let scope = self.arena.alloc(ScopeGuard::new());
        self.scopes.push(scope);
    }

    pub fn outer_scope(&mut self, ct: u32) -> &mut ScopeGuard {
        let length = self.scopes.len();
        self.scopes.get_mut(length - 1 - ct as usize).unwrap()
    }

    pub fn pop_scope(&mut self) {
        trace!("Validator::pop_scope");
        self.scopes.pop().take();
    }

    #[inline]
    pub fn scope(&self) -> &ScopeGuard {
        self.scopes.last().unwrap()
    }

    #[inline]
    pub fn scope_mut(&mut self) -> &mut ScopeGuard {
        self.scopes.last_mut().unwrap()
    }

    #[inline]
    pub fn with_scope<T>(&mut self, f: impl FnOnce(&mut ScopeGuard) -> T) -> T {
        let scope = self.scope_mut();
        f(scope)
    }

    pub fn new_type(&mut self, ty: Type) -> TypeId {
        let id = TypeId::new();
        self.types.insert(id, ty);
        id
    }

    pub fn get_type(&self, id: TypeId) -> ParseResult<&Type> {
        Ok(self.types.get(&id).unwrap())
    }
}
