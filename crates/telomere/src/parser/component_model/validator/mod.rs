mod scope;
mod state;

pub use scope::ScopeGuard;
pub use state::ValidatorState;
use std::cell::{RefCell, RefMut};
use tracing::trace;
use typed_arena::Arena;

pub struct Validator<'a> {
    arena: &'a Arena<ScopeGuard<'a>>,
    pub(crate) current: Option<&'a mut ScopeGuard<'a>>,
}

impl<'a> Validator<'a> {
    pub fn new(arena: &'a Arena<ScopeGuard<'a>>) -> Self {
        let current = arena.alloc(ScopeGuard::new(None));
        Self {
            arena,
            current: Some(current),
        }
    }

    pub fn new_scope(&mut self) {
        trace!("Validator::new_scope");
        if self.current.is_some() {
            let scope = self.arena.alloc(ScopeGuard::new(self.current.take()));
            self.current = Some(scope);
        } else {
            let scope = self.arena.alloc(ScopeGuard::new(None));
            self.current = Some(scope);
        }
    }

    pub fn outer_scope(&mut self, ct: u32) -> &mut ScopeGuard<'a> {
        let mut scope = self.current.as_mut().unwrap();
        for _ in 0..ct {
            if let Some(parent) = scope.parent.as_mut() {
                scope = parent;
            } else {
                panic!("No outer scope available");
            }
        }
        scope
    }

    pub fn merge_types_into_parent(&mut self) {
        let Some(ref mut scope) = self.current else {
            panic!("No scope to merge");
        };
        let mapping = scope.type_mapping.clone();
        let Some(ref mut parent) = scope.parent else {
            panic!("No parent scope to merge into");
        };
        parent.merge_type(mapping);
        parent.uf.merge(&scope.uf);
    }

    pub fn merge_globals_into_parent(&mut self) {
        let Some(ref mut scope) = self.current else {
            panic!("No scope to merge");
        };
        let Some(ref mut parent) = scope.parent else {
            panic!("No parent scope to merge into");
        };
        // Merge globals from current scope into parent scope
        parent.instances.merge(&scope.instances);
        parent.components.merge(&scope.components);
    }

    pub fn pop_scope(&mut self) {
        trace!("Validator::pop_scope");
        if let Some(scope) = self.current.take() {
            self.current = scope.parent.take();
        } else {
            panic!("No scope to pop");
        }
    }

    #[inline]
    pub fn scope(&self) -> &ScopeGuard<'a> {
        self.current.as_ref().unwrap()
    }

    #[inline]
    pub fn scope_mut(&mut self) -> &mut ScopeGuard<'a> {
        self.current.as_mut().unwrap()
    }

    #[inline]
    pub fn with_scope<T>(&mut self, f: impl FnOnce(&mut ScopeGuard<'a>) -> T) -> T {
        let scope = self.scope_mut();
        f(scope)
    }
}
