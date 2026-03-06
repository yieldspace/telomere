mod scope;
mod state;

use super::ComponentParseError;
use crate::component::decoder::ParseResult;
use crate::component::ir::types::{
    ComponentExportType, ComponentType, FuncType, Generic, GenericBound, InstanceExportType,
    InstanceType, Type,
};
use crate::component::ir::TypeId;
pub use scope::ExportInfo;
pub use scope::ScopeGuard;
pub use state::ParseState;
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
        for (name, info) in &scope.exports {
            let export_ty = match &info {
                ExportInfo::Component(id) => ComponentExportType::Component(*id),
                ExportInfo::TypeEq(id) => ComponentExportType::Type(*id),
                ExportInfo::Instance(id) => ComponentExportType::Instance(*id),
                ExportInfo::Func(id) => ComponentExportType::Func(*id),
                ExportInfo::TypeSub => {
                    let id = TypeId::new();
                    self.types
                        .insert(id, Type::Generic(Generic::new(GenericBound::Sub)));
                    ComponentExportType::Type(id)
                }
            };
            exports.insert(name.clone(), export_ty);
        }
        ComponentType {
            imports,
            exports,
            generics_replacing_program: scope.generics_replace_program.clone(),
        }
    }
    pub fn make_instance(&mut self) -> InstanceType {
        let scope = self.scopes.last().unwrap();
        let mut exports = HashMap::new();
        tracing::trace!("make_instance {:?}", scope.exports);
        for (name, info) in &scope.exports {
            let export_ty = match info {
                ExportInfo::Component(type_id) => InstanceExportType::Component(*type_id),
                ExportInfo::Instance(type_id) => InstanceExportType::Instance(*type_id),
                ExportInfo::Func(type_id) => InstanceExportType::Func(*type_id),
                ExportInfo::TypeEq(type_id) => InstanceExportType::Type(*type_id),
                ExportInfo::TypeSub => {
                    let id = TypeId::new();
                    self.types
                        .insert(id, Type::Generic(Generic::new(GenericBound::Sub)));
                    InstanceExportType::Type(id)
                }
            };
            exports.insert(name.clone(), export_ty);
        }
        InstanceType { exports }
    }

    pub fn get_func_type(&self, id: TypeId) -> ParseResult<&FuncType> {
        if let Type::Func(ty) = self.get_type(id)? {
            Ok(ty)
        } else {
            Err(ComponentParseError::TypeMismatch(
                "Type ID does not refer to any func".to_owned(),
            ))?
        }
    }
}
