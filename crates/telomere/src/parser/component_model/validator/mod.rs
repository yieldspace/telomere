mod scope;
mod state;

use crate::component_model::types::{ComponentExportType, ComponentType, Generic, GenericBound, InstanceExportType, InstanceType, Type};
use crate::component_model::{ExternDesc, TypeId};
use crate::parser::component_model::ParseResult;
pub use scope::ScopeGuard;
pub use state::ParseState;
use std::collections::HashMap;
use tracing::trace;
use typed_arena::Arena;

use super::ComponentParseError;

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
        let _ = self.scopes.pop();
    }

    #[inline]
    pub fn scope(&self) -> &ScopeGuard {
        self.scopes.last().unwrap()
    }

    #[inline]
    pub fn scope_mut(&mut self) -> &mut ScopeGuard {
        self.scopes.last_mut().unwrap()
    }

    pub fn new_type(&mut self, ty: Type) -> TypeId {
        let id = TypeId::new();
        self.types.insert(id, ty);
        id
    }

    pub fn get_type(&self, id: TypeId) -> ParseResult<&Type> {
        Ok(self.types.get(&id).unwrap())
    }

    pub fn get_component_type(&self, id: TypeId) -> ParseResult<&ComponentType> {
        if let Type::Component(component_ty) = self.get_type(id)? {
            Ok(component_ty)
        } else {
            Err(ComponentParseError::TypeMismatch(
                "Type ID does not refer to any component".to_owned(),
            ))?
        }
    }

    pub fn get_instance_type(&self, id: TypeId) -> ParseResult<&InstanceType> {
        if let Type::Instance(ty) = self.get_type(id)? {
            Ok(ty)
        } else {
            Err(ComponentParseError::TypeMismatch(
                "Type ID does not refer to any instance".to_owned(),
            ))?
        }
    }
    pub fn make_component(&mut self) -> ComponentType {
        let scope = self.scopes.last().unwrap();
        let imports = scope.imports.clone();
        let mut exports = HashMap::new();
        for (name, desc) in &scope.exports {
            let export_ty = match desc {
                ExternDesc::Component(id) => ComponentExportType::Component(*id),
                ExternDesc::Eq(id) => ComponentExportType::Type(*id),
                ExternDesc::Instance(id) => ComponentExportType::Instance(*id),
                ExternDesc::Func(id) => ComponentExportType::Type(*id), // FIXME: ?
                ExternDesc::Sub => {
                    let id = TypeId::new();
                    self.types.insert(id,Type::Generic(Generic::new(GenericBound::Sub)));
                    ComponentExportType::NewResource(id)
                }
            };
            exports.insert(name.clone(), export_ty);
        }
        ComponentType { imports, exports }
    }
    pub fn make_instance(&mut self) -> InstanceType {
        let scope = self.scopes.last().unwrap();
        let mut exports = HashMap::new();
        for (name, desc) in &scope.exports {
            let export_ty = match desc {
                ExternDesc::Component(id) => InstanceExportType::Component(*id),
                ExternDesc::Eq(id) => InstanceExportType::(*id),
                ExternDesc::Instance(id) => InstanceExportType::Instance(*id),
                ExternDesc::Func(id) => InstanceExportType::Func(*id), // FIXME: ?
                ExternDesc::Sub => {
                    
                    InstanceExportType::Resource(id)
                }
            };
            exports.insert(name.clone(), export_ty);
        }
        InstanceType { exports }
    }
}
