mod scope;
mod state;

use super::ComponentParseError;
use crate::component::decoder::ParseResult;
use crate::component::ir::types::{
    ComponentExportType, ComponentImportType, ComponentType, DefValType, FuncType, Generic,
    GenericBound, InstanceExportType, InstanceType, Type, ValType,
};
use crate::component::ir::{ResourceId, ScopeId, TypeId};
pub use scope::ExportInfo;
pub use scope::ScopeGuard;
pub use state::ParseState;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use tracing::trace;
use typed_arena::Arena;

pub struct Validator<'a> {
    arena: &'a Arena<ScopeGuard>,
    scopes: Vec<&'a mut ScopeGuard>,
    scope_kinds: Vec<ScopeKind>,
    types: HashMap<TypeId, Type>,
    validated_component_like: RefCell<HashSet<TypeId>>,
    type_sizes: RefCell<HashMap<TypeId, u64>>,
}

#[derive(Copy, Clone)]
enum SurfaceRole {
    Import,
    Export,
}

impl SurfaceRole {
    fn noun(self) -> &'static str {
        match self {
            SurfaceRole::Import => "import",
            SurfaceRole::Export => "export",
        }
    }
}

#[derive(Copy, Clone)]
enum ScopeKind {
    Concrete,
    Type,
}

impl<'a> Validator<'a> {
    pub fn new(arena: &'a Arena<ScopeGuard>) -> Self {
        let current = arena.alloc(ScopeGuard::new(ScopeId::new(0)));
        Self {
            arena,
            scopes: vec![current],
            scope_kinds: vec![ScopeKind::Concrete],
            types: HashMap::new(),
            validated_component_like: RefCell::new(HashSet::new()),
            type_sizes: RefCell::new(HashMap::new()),
        }
    }

    pub fn push_scope(&mut self) {
        trace!("Validator::push_scope");
        let scope = self
            .arena
            .alloc(ScopeGuard::new(ScopeId::new(self.scopes.len() as u32)));
        self.scopes.push(scope);
        self.scope_kinds.push(ScopeKind::Concrete);
    }

    pub fn push_type_scope(&mut self) {
        trace!("Validator::push_type_scope");
        let scope = self
            .arena
            .alloc(ScopeGuard::new(ScopeId::new(self.scopes.len() as u32)));
        self.scopes.push(scope);
        self.scope_kinds.push(ScopeKind::Type);
    }

    pub fn push_nested_type_scope(&mut self) {
        trace!("Validator::push_nested_type_scope");
        let scope = self
            .arena
            .alloc(ScopeGuard::new(ScopeId::new(self.scopes.len() as u32)));
        self.scopes.push(scope);
        self.scope_kinds.push(ScopeKind::Type);
    }

    pub fn outer_scope(&mut self, ct: u32) -> ParseResult<&mut ScopeGuard> {
        let index = self
            .scopes
            .len()
            .checked_sub(1 + ct as usize)
            .ok_or(ComponentParseError::InvalidScope)?;
        self.scopes
            .get_mut(index)
            .map(|scope| &mut **scope)
            .ok_or(ComponentParseError::InvalidScope)
    }

    pub fn outer_type_scope(&mut self, ct: u32) -> ParseResult<&mut ScopeGuard> {
        let index = self
            .scopes
            .len()
            .checked_sub(ct as usize)
            .ok_or(ComponentParseError::InvalidScope)?;
        self.scopes
            .get_mut(index)
            .map(|scope| &mut **scope)
            .ok_or(ComponentParseError::InvalidScope)
    }

    pub fn pop_scope(&mut self) {
        trace!("Validator::pop_scope");
        let _ = self.scopes.pop();
        let _ = self.scope_kinds.pop();
    }

    #[inline]
    pub fn scope(&self) -> &ScopeGuard {
        self.scopes.last().unwrap()
    }

    #[inline]
    pub fn scope_mut(&mut self) -> &mut ScopeGuard {
        self.scopes.last_mut().unwrap()
    }

    pub fn in_concrete_scope(&self) -> bool {
        matches!(self.scope_kinds.last(), Some(ScopeKind::Concrete))
    }

    pub fn current_scope_id(&self) -> ScopeId {
        self.scope().scope_id
    }

    pub fn new_type(&mut self, ty: Type) -> TypeId {
        let id = TypeId::new();
        self.types.insert(id, ty);
        id
    }

    pub fn get_type(&self, id: TypeId) -> ParseResult<&Type> {
        self.types
            .get(&id)
            .ok_or(ComponentParseError::TypeNotFound(id))
    }

    pub fn snapshot_types(&self) -> HashMap<TypeId, Type> {
        self.types.clone()
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
                ExportInfo::CoreModule(ty) => ComponentExportType::CoreModule(ty.clone()),
                ExportInfo::Component(id) => ComponentExportType::Component(*id),
                ExportInfo::TypeEq(id) => ComponentExportType::Type(*id),
                ExportInfo::Instance(id) => ComponentExportType::Instance(*id),
                ExportInfo::Func(id) => ComponentExportType::Func(*id),
                ExportInfo::TypeSub(id) => ComponentExportType::Type(*id),
            };
            exports.insert(name.clone(), export_ty);
        }
        ComponentType {
            import_order: scope
                .import_names
                .iter()
                .map(|name| name.original.clone())
                .collect(),
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
                ExportInfo::CoreModule(ty) => InstanceExportType::CoreModule(ty.clone()),
                ExportInfo::Component(type_id) => InstanceExportType::Component(*type_id),
                ExportInfo::Instance(type_id) => InstanceExportType::Instance(*type_id),
                ExportInfo::Func(type_id) => InstanceExportType::Func(*type_id),
                ExportInfo::TypeEq(type_id) => InstanceExportType::Type(*type_id),
                ExportInfo::TypeSub(id) => InstanceExportType::Type(*id),
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

    pub fn instantiate_type_id(
        &mut self,
        type_id: TypeId,
        unified: &mut HashMap<TypeId, TypeId>,
    ) -> ParseResult<TypeId> {
        if let Some(mapped) = unified.get(&type_id).copied() {
            return Ok(mapped);
        }

        let ty = self.get_type(type_id)?.clone();
        let cloned = self.instantiate_type(&ty, unified)?;
        let new_type_id = self.new_type(cloned);
        self.validate_effective_type_size(new_type_id)?;
        unified.insert(type_id, new_type_id);
        Ok(new_type_id)
    }

    pub fn instantiate_sub_resource_type(
        &mut self,
        type_id: TypeId,
        unified: &mut HashMap<TypeId, TypeId>,
    ) -> ParseResult<TypeId> {
        if let Some(mapped) = unified.get(&type_id).copied() {
            return Ok(mapped);
        }

        let ty = self.get_type(type_id)?;
        if !matches!(
            ty,
            Type::Generic(Generic {
                bound: GenericBound::Sub,
                ..
            }) | Type::Resource(_)
        ) {
            return Err(ComponentParseError::TypeMismatch(
                "expected resource".to_owned(),
            ));
        }

        let new_type_id = self.new_type(Type::Resource(ResourceId::synthetic()));
        self.validate_effective_type_size(new_type_id)?;
        unified.insert(type_id, new_type_id);
        Ok(new_type_id)
    }

    pub fn freshen_import_type_id(&mut self, type_id: TypeId) -> ParseResult<TypeId> {
        let mut unified = HashMap::new();
        self.freshen_import_type_id_with_map(type_id, &mut unified)
    }

    pub fn resolve_surface_type_id(
        &mut self,
        type_id: TypeId,
        unified: &mut HashMap<TypeId, TypeId>,
    ) -> ParseResult<TypeId> {
        self.freshen_import_type_id_with_map(type_id, unified)
    }

    fn freshen_import_type_id_with_map(
        &mut self,
        type_id: TypeId,
        unified: &mut HashMap<TypeId, TypeId>,
    ) -> ParseResult<TypeId> {
        if let Some(mapped) = unified.get(&type_id).copied() {
            return Ok(mapped);
        }

        let ty = self.get_type(type_id)?.clone();
        let cloned = self.freshen_import_type(&ty, unified)?;
        let new_type_id = self.new_type(cloned);
        self.validate_effective_type_size(new_type_id)?;
        unified.insert(type_id, new_type_id);
        Ok(new_type_id)
    }

    fn instantiate_type(
        &mut self,
        ty: &Type,
        unified: &mut HashMap<TypeId, TypeId>,
    ) -> ParseResult<Type> {
        Ok(match ty {
            Type::DefVal(def) => Type::DefVal(self.instantiate_defval(def, unified)?),
            Type::Generic(Generic {
                bound: GenericBound::Eq(inner),
                ..
            }) => {
                let inner = self.instantiate_type_id(*inner, unified)?;
                Type::Generic(Generic::new(GenericBound::Eq(inner)))
            }
            Type::Generic(Generic {
                bound: GenericBound::Sub,
                ..
            }) => Type::Generic(Generic::new(GenericBound::Sub)),
            Type::Func(func_ty) => Type::Func(self.instantiate_func(func_ty, unified)?),
            Type::Resource(_) => Type::Resource(ResourceId::synthetic()),
            Type::Component(component_ty) => {
                Type::Component(self.instantiate_component_type(component_ty, unified)?)
            }
            Type::Instance(instance_ty) => {
                Type::Instance(self.instantiate_instance_type(instance_ty, unified)?)
            }
        })
    }

    fn freshen_import_type(
        &mut self,
        ty: &Type,
        unified: &mut HashMap<TypeId, TypeId>,
    ) -> ParseResult<Type> {
        Ok(match ty {
            Type::DefVal(def) => Type::DefVal(self.freshen_import_defval(def, unified)?),
            Type::Generic(Generic {
                bound: GenericBound::Eq(inner),
                ..
            }) => Type::Generic(Generic::new(GenericBound::Eq(
                self.freshen_import_type_id_with_map(*inner, unified)?,
            ))),
            Type::Generic(Generic {
                bound: GenericBound::Sub,
                ..
            }) => Type::Generic(Generic::new(GenericBound::Sub)),
            Type::Func(func_ty) => Type::Func(self.freshen_import_func(func_ty, unified)?),
            Type::Resource(resource) => Type::Resource(*resource),
            Type::Component(component_ty) => {
                Type::Component(self.freshen_import_component_type(component_ty, unified)?)
            }
            Type::Instance(instance_ty) => {
                Type::Instance(self.freshen_import_instance_type(instance_ty, unified)?)
            }
        })
    }

    fn instantiate_component_type(
        &mut self,
        ty: &ComponentType,
        unified: &mut HashMap<TypeId, TypeId>,
    ) -> ParseResult<ComponentType> {
        let mut imports = HashMap::new();
        for (name, import) in &ty.imports {
            let import = match import {
                ComponentImportType::CoreModule(module_ty) => {
                    ComponentImportType::CoreModule(module_ty.clone())
                }
                ComponentImportType::Type { type_id, generic } => {
                    let type_id = self.instantiate_type_id(*type_id, unified)?;
                    let generic = match &generic.bound {
                        GenericBound::Eq(inner) => Generic::new(GenericBound::Eq(
                            self.instantiate_type_id(*inner, unified)?,
                        )),
                        GenericBound::Sub => Generic::new(GenericBound::Sub),
                    };
                    ComponentImportType::Type { type_id, generic }
                }
            };
            imports.insert(name.clone(), import);
        }

        let mut exports = HashMap::new();
        for (name, export) in &ty.exports {
            let export = match export {
                ComponentExportType::CoreModule(module_ty) => {
                    ComponentExportType::CoreModule(module_ty.clone())
                }
                ComponentExportType::Component(type_id) => {
                    ComponentExportType::Component(self.instantiate_type_id(*type_id, unified)?)
                }
                ComponentExportType::Instance(type_id) => {
                    ComponentExportType::Instance(self.instantiate_type_id(*type_id, unified)?)
                }
                ComponentExportType::Type(type_id) => {
                    ComponentExportType::Type(self.instantiate_type_id(*type_id, unified)?)
                }
                ComponentExportType::Func(type_id) => {
                    ComponentExportType::Func(self.instantiate_type_id(*type_id, unified)?)
                }
            };
            exports.insert(name.clone(), export);
        }

        Ok(ComponentType {
            import_order: ty.import_order.clone(),
            imports,
            exports,
            generics_replacing_program: ty.generics_replacing_program.clone(),
        })
    }

    fn freshen_import_component_type(
        &mut self,
        ty: &ComponentType,
        unified: &mut HashMap<TypeId, TypeId>,
    ) -> ParseResult<ComponentType> {
        let mut imports = HashMap::new();
        for (name, import) in &ty.imports {
            let import = match import {
                ComponentImportType::CoreModule(module_ty) => {
                    ComponentImportType::CoreModule(module_ty.clone())
                }
                ComponentImportType::Type { type_id, generic } => {
                    let type_id = self.freshen_import_type_id_with_map(*type_id, unified)?;
                    let generic = match &generic.bound {
                        GenericBound::Eq(inner) => Generic::new(GenericBound::Eq(
                            self.freshen_import_type_id_with_map(*inner, unified)?,
                        )),
                        GenericBound::Sub => Generic::new(GenericBound::Sub),
                    };
                    ComponentImportType::Type { type_id, generic }
                }
            };
            imports.insert(name.clone(), import);
        }

        let mut exports = HashMap::new();
        for (name, export) in &ty.exports {
            let export = match export {
                ComponentExportType::CoreModule(module_ty) => {
                    ComponentExportType::CoreModule(module_ty.clone())
                }
                ComponentExportType::Component(type_id) => ComponentExportType::Component(
                    self.freshen_import_type_id_with_map(*type_id, unified)?,
                ),
                ComponentExportType::Instance(type_id) => ComponentExportType::Instance(
                    self.freshen_import_type_id_with_map(*type_id, unified)?,
                ),
                ComponentExportType::Type(type_id) => ComponentExportType::Type(
                    self.freshen_import_type_id_with_map(*type_id, unified)?,
                ),
                ComponentExportType::Func(type_id) => ComponentExportType::Func(
                    self.freshen_import_type_id_with_map(*type_id, unified)?,
                ),
            };
            exports.insert(name.clone(), export);
        }

        Ok(ComponentType {
            import_order: ty.import_order.clone(),
            imports,
            exports,
            generics_replacing_program: ty.generics_replacing_program.clone(),
        })
    }

    fn instantiate_instance_type(
        &mut self,
        ty: &InstanceType,
        unified: &mut HashMap<TypeId, TypeId>,
    ) -> ParseResult<InstanceType> {
        let mut exports = HashMap::new();
        for (name, export) in &ty.exports {
            let export = match export {
                InstanceExportType::CoreModule(module_ty) => {
                    InstanceExportType::CoreModule(module_ty.clone())
                }
                InstanceExportType::Func(type_id) => {
                    InstanceExportType::Func(self.instantiate_type_id(*type_id, unified)?)
                }
                InstanceExportType::Component(type_id) => {
                    InstanceExportType::Component(self.instantiate_type_id(*type_id, unified)?)
                }
                InstanceExportType::Instance(type_id) => {
                    InstanceExportType::Instance(self.instantiate_type_id(*type_id, unified)?)
                }
                InstanceExportType::Type(type_id) => {
                    InstanceExportType::Type(self.instantiate_type_id(*type_id, unified)?)
                }
            };
            exports.insert(name.clone(), export);
        }
        Ok(InstanceType { exports })
    }

    fn freshen_import_instance_type(
        &mut self,
        ty: &InstanceType,
        unified: &mut HashMap<TypeId, TypeId>,
    ) -> ParseResult<InstanceType> {
        let mut exports = HashMap::new();
        for (name, export) in &ty.exports {
            let export = match export {
                InstanceExportType::CoreModule(module_ty) => {
                    InstanceExportType::CoreModule(module_ty.clone())
                }
                InstanceExportType::Func(type_id) => InstanceExportType::Func(
                    self.freshen_import_type_id_with_map(*type_id, unified)?,
                ),
                InstanceExportType::Component(type_id) => InstanceExportType::Component(
                    self.freshen_import_type_id_with_map(*type_id, unified)?,
                ),
                InstanceExportType::Instance(type_id) => InstanceExportType::Instance(
                    self.freshen_import_type_id_with_map(*type_id, unified)?,
                ),
                InstanceExportType::Type(type_id) => InstanceExportType::Type(
                    self.freshen_import_type_id_with_map(*type_id, unified)?,
                ),
            };
            exports.insert(name.clone(), export);
        }
        Ok(InstanceType { exports })
    }

    fn instantiate_func(
        &mut self,
        ty: &FuncType,
        unified: &mut HashMap<TypeId, TypeId>,
    ) -> ParseResult<FuncType> {
        let params = ty
            .params
            .iter()
            .map(|param| self.instantiate_valtype(param, unified))
            .collect::<ParseResult<Vec<_>>>()?;
        let result = ty
            .result
            .as_ref()
            .map(|result| self.instantiate_valtype(result, unified))
            .transpose()?;
        Ok(FuncType {
            params,
            param_names: ty.param_names.clone(),
            result,
        })
    }

    fn freshen_import_func(
        &mut self,
        ty: &FuncType,
        unified: &mut HashMap<TypeId, TypeId>,
    ) -> ParseResult<FuncType> {
        let params = ty
            .params
            .iter()
            .map(|param| self.freshen_import_valtype(param, unified))
            .collect::<ParseResult<Vec<_>>>()?;
        let result = ty
            .result
            .as_ref()
            .map(|result| self.freshen_import_valtype(result, unified))
            .transpose()?;
        Ok(FuncType {
            params,
            param_names: ty.param_names.clone(),
            result,
        })
    }

    fn instantiate_valtype(
        &mut self,
        ty: &ValType,
        unified: &mut HashMap<TypeId, TypeId>,
    ) -> ParseResult<ValType> {
        Ok(match ty {
            ValType::Primitive(prim) => ValType::Primitive(prim.clone()),
            ValType::Type(type_id) => ValType::Type(self.instantiate_type_id(*type_id, unified)?),
        })
    }

    fn freshen_import_valtype(
        &mut self,
        ty: &ValType,
        unified: &mut HashMap<TypeId, TypeId>,
    ) -> ParseResult<ValType> {
        Ok(match ty {
            ValType::Primitive(prim) => ValType::Primitive(prim.clone()),
            ValType::Type(type_id) => {
                ValType::Type(self.freshen_import_type_id_with_map(*type_id, unified)?)
            }
        })
    }

    fn instantiate_defval(
        &mut self,
        ty: &DefValType,
        unified: &mut HashMap<TypeId, TypeId>,
    ) -> ParseResult<DefValType> {
        Ok(match ty {
            DefValType::Primitive(prim) => DefValType::Primitive(prim.clone()),
            DefValType::Record(fields) => DefValType::Record(
                fields
                    .iter()
                    .map(|field| {
                        Ok(crate::component::ir::types::LabelValType::new(
                            field.label.clone(),
                            self.instantiate_valtype(&field.ty, unified)?,
                        ))
                    })
                    .collect::<ParseResult<Vec<_>>>()?,
            ),
            DefValType::Variant(cases) => DefValType::Variant(
                cases
                    .iter()
                    .map(|case| {
                        Ok(crate::component::ir::types::Case::new(
                            case.label.clone(),
                            case.ty
                                .as_ref()
                                .map(|ty| self.instantiate_valtype(ty, unified))
                                .transpose()?,
                        ))
                    })
                    .collect::<ParseResult<Vec<_>>>()?,
            ),
            DefValType::List(ty, len) => {
                DefValType::List(self.instantiate_valtype(ty, unified)?, *len)
            }
            DefValType::Own(type_id) => {
                DefValType::Own(self.instantiate_type_id(*type_id, unified)?)
            }
            DefValType::Borrow(type_id) => {
                DefValType::Borrow(self.instantiate_type_id(*type_id, unified)?)
            }
        })
    }

    fn freshen_import_defval(
        &mut self,
        ty: &DefValType,
        unified: &mut HashMap<TypeId, TypeId>,
    ) -> ParseResult<DefValType> {
        Ok(match ty {
            DefValType::Primitive(prim) => DefValType::Primitive(prim.clone()),
            DefValType::Record(fields) => DefValType::Record(
                fields
                    .iter()
                    .map(|field| {
                        Ok(crate::component::ir::types::LabelValType::new(
                            field.label.clone(),
                            self.freshen_import_valtype(&field.ty, unified)?,
                        ))
                    })
                    .collect::<ParseResult<Vec<_>>>()?,
            ),
            DefValType::Variant(cases) => DefValType::Variant(
                cases
                    .iter()
                    .map(|case| {
                        Ok(crate::component::ir::types::Case::new(
                            case.label.clone(),
                            case.ty
                                .as_ref()
                                .map(|ty| self.freshen_import_valtype(ty, unified))
                                .transpose()?,
                        ))
                    })
                    .collect::<ParseResult<Vec<_>>>()?,
            ),
            DefValType::List(ty, len) => {
                DefValType::List(self.freshen_import_valtype(ty, unified)?, *len)
            }
            DefValType::Own(type_id) => {
                DefValType::Own(self.freshen_import_type_id_with_map(*type_id, unified)?)
            }
            DefValType::Borrow(type_id) => {
                DefValType::Borrow(self.freshen_import_type_id_with_map(*type_id, unified)?)
            }
        })
    }

    pub fn validate_effective_type_size(&self, type_id: TypeId) -> ParseResult<()> {
        const EFFECTIVE_TYPE_SIZE_LIMIT: u64 = 1_000_000;

        let mut visiting = HashSet::new();
        let size = self.compute_effective_type_size(type_id, &mut visiting)?;
        if size > EFFECTIVE_TYPE_SIZE_LIMIT {
            return Err(ComponentParseError::TypeMismatch(
                "effective type size exceeds the limit".to_owned(),
            ));
        }
        Ok(())
    }

    pub fn validate_current_component_resources(&self, type_id: TypeId) -> ParseResult<()> {
        if self.type_refs_foreign_resource(type_id, self.current_scope_id(), &mut HashSet::new())? {
            return Err(ComponentParseError::TypeMismatch(
                "refers to resources not defined in the current component".to_owned(),
            ));
        }
        Ok(())
    }

    pub fn validate_component_surface(&self, ty: &ComponentType) -> ParseResult<()> {
        let mut seen = HashSet::new();
        self.validate_component_surface_inner(ty, &HashSet::new(), &mut seen)
    }

    pub fn validate_instance_surface(&self, ty: &InstanceType) -> ParseResult<()> {
        let mut seen = HashSet::new();
        self.validate_instance_surface_inner(ty, &HashSet::new(), &mut seen)
    }

    pub fn validate_component_type_definition(&self, type_id: TypeId) -> ParseResult<()> {
        if self.validated_component_like.borrow().contains(&type_id) {
            return Ok(());
        }
        let Type::Component(component_ty) = self.get_type(type_id)?.clone() else {
            return Err(ComponentParseError::TypeMismatch(
                "Type ID does not refer to any component".to_owned(),
            ));
        };
        let mut seen = HashSet::new();
        self.validate_component_surface_inner(&component_ty, &HashSet::new(), &mut seen)?;
        self.validated_component_like.borrow_mut().insert(type_id);
        Ok(())
    }

    pub fn validate_instance_type_definition(&self, type_id: TypeId) -> ParseResult<()> {
        if self.validated_component_like.borrow().contains(&type_id) {
            return Ok(());
        }
        let Type::Instance(instance_ty) = self.get_type(type_id)?.clone() else {
            return Err(ComponentParseError::TypeMismatch(
                "Type ID does not refer to any instance".to_owned(),
            ));
        };
        let mut seen = HashSet::new();
        self.validate_instance_surface_inner(&instance_ty, &HashSet::new(), &mut seen)?;
        self.validated_component_like.borrow_mut().insert(type_id);
        Ok(())
    }

    fn validate_component_surface_inner(
        &self,
        ty: &ComponentType,
        inherited_visible: &HashSet<TypeId>,
        seen: &mut HashSet<TypeId>,
    ) -> ParseResult<()> {
        let import_type_ids = self.collect_component_visible_types(ty, SurfaceRole::Import)?;
        let export_type_ids =
            ty.exports
                .iter()
                .fold(import_type_ids.clone(), |mut visible, (_, export)| {
                    self.extend_export_visible_types(export, &mut visible);
                    visible
                });
        let mut import_visible = inherited_visible.clone();
        import_visible.extend(import_type_ids);
        let mut export_visible = import_visible.clone();
        export_visible.extend(export_type_ids);

        for import in ty.imports.values() {
            self.validate_component_import_surface(import, &import_visible, seen)?;
        }
        for export in ty.exports.values() {
            self.validate_component_export_surface(export, &export_visible, seen)?;
        }
        Ok(())
    }

    fn validate_instance_surface_inner(
        &self,
        ty: &InstanceType,
        inherited_visible: &HashSet<TypeId>,
        seen: &mut HashSet<TypeId>,
    ) -> ParseResult<()> {
        let mut visible = inherited_visible.clone();
        visible.extend(self.collect_instance_visible_types(ty)?);
        for export in ty.exports.values() {
            self.validate_instance_export_surface(export, &visible, seen)?;
        }
        Ok(())
    }

    fn validate_component_import_surface(
        &self,
        import: &ComponentImportType,
        visible: &HashSet<TypeId>,
        seen: &mut HashSet<TypeId>,
    ) -> ParseResult<()> {
        match import {
            ComponentImportType::CoreModule(_) => Ok(()),
            ComponentImportType::Type { type_id, .. } => {
                self.validate_type_root(*type_id, visible, SurfaceRole::Import, seen)
            }
        }
    }

    fn validate_component_export_surface(
        &self,
        export: &ComponentExportType,
        visible: &HashSet<TypeId>,
        seen: &mut HashSet<TypeId>,
    ) -> ParseResult<()> {
        match export {
            ComponentExportType::CoreModule(_) => Ok(()),
            ComponentExportType::Component(type_id) => {
                self.validate_component_root(*type_id, visible, SurfaceRole::Export, seen)
            }
            ComponentExportType::Instance(type_id) => {
                self.validate_instance_root(*type_id, visible, SurfaceRole::Export, seen)
            }
            ComponentExportType::Type(type_id) => {
                self.validate_type_root(*type_id, visible, SurfaceRole::Export, seen)
            }
            ComponentExportType::Func(type_id) => {
                self.validate_func_root(*type_id, visible, SurfaceRole::Export, seen)
            }
        }
    }

    fn validate_instance_export_surface(
        &self,
        export: &InstanceExportType,
        visible: &HashSet<TypeId>,
        seen: &mut HashSet<TypeId>,
    ) -> ParseResult<()> {
        match export {
            InstanceExportType::CoreModule(_) => Ok(()),
            InstanceExportType::Component(type_id) => {
                self.validate_component_root(*type_id, visible, SurfaceRole::Export, seen)
            }
            InstanceExportType::Instance(type_id) => {
                self.validate_instance_root(*type_id, visible, SurfaceRole::Export, seen)
            }
            InstanceExportType::Type(type_id) => {
                self.validate_type_root(*type_id, visible, SurfaceRole::Export, seen)
            }
            InstanceExportType::Func(type_id) => {
                self.validate_func_root(*type_id, visible, SurfaceRole::Export, seen)
            }
        }
    }

    fn validate_type_root(
        &self,
        type_id: TypeId,
        visible: &HashSet<TypeId>,
        role: SurfaceRole,
        seen: &mut HashSet<TypeId>,
    ) -> ParseResult<()> {
        if let Type::DefVal(def) = self.get_type(type_id)? {
            if self.defval_contains_resource_handle(def, &mut HashSet::new())? {
                return Err(ComponentParseError::TypeMismatch(format!(
                    "type not valid to be used as {}",
                    role.noun()
                )));
            }
        }
        self.validate_type_definition(type_id, visible, seen)
            .map_err(|error| {
                ComponentParseError::TypeMismatch(format!(
                    "type not valid to be used as {}: {}",
                    role.noun(),
                    error
                ))
            })
    }

    fn validate_func_root(
        &self,
        type_id: TypeId,
        visible: &HashSet<TypeId>,
        role: SurfaceRole,
        seen: &mut HashSet<TypeId>,
    ) -> ParseResult<()> {
        let Type::Func(func_ty) = self.get_type(type_id)?.clone() else {
            return Err(ComponentParseError::TypeMismatch(format!(
                "func not valid to be used as {}",
                role.noun()
            )));
        };
        self.validate_func_definition(&func_ty, visible, seen)
            .map_err(|error| {
                ComponentParseError::TypeMismatch(format!(
                    "func not valid to be used as {}: {}",
                    role.noun(),
                    error
                ))
            })
    }

    fn validate_component_root(
        &self,
        type_id: TypeId,
        visible: &HashSet<TypeId>,
        role: SurfaceRole,
        seen: &mut HashSet<TypeId>,
    ) -> ParseResult<()> {
        if !seen.insert(type_id) {
            return Ok(());
        }
        let Type::Component(component_ty) = self.get_type(type_id)?.clone() else {
            return Err(ComponentParseError::TypeMismatch(format!(
                "component not valid to be used as {}",
                role.noun()
            )));
        };
        let result = self
            .validate_component_surface_inner(&component_ty, visible, seen)
            .map_err(|error| {
                ComponentParseError::TypeMismatch(format!(
                    "component not valid to be used as {}: {}",
                    role.noun(),
                    error
                ))
            });
        seen.remove(&type_id);
        result
    }

    fn validate_instance_root(
        &self,
        type_id: TypeId,
        visible: &HashSet<TypeId>,
        role: SurfaceRole,
        seen: &mut HashSet<TypeId>,
    ) -> ParseResult<()> {
        if !seen.insert(type_id) {
            return Ok(());
        }
        let Type::Instance(instance_ty) = self.get_type(type_id)?.clone() else {
            return Err(ComponentParseError::TypeMismatch(format!(
                "instance not valid to be used as {}",
                role.noun()
            )));
        };
        let result = self
            .validate_instance_surface_inner(&instance_ty, visible, seen)
            .map_err(|error| {
                ComponentParseError::TypeMismatch(format!(
                    "instance not valid to be used as {}: {}",
                    role.noun(),
                    error
                ))
            });
        seen.remove(&type_id);
        result
    }

    fn validate_type_definition(
        &self,
        type_id: TypeId,
        visible: &HashSet<TypeId>,
        seen: &mut HashSet<TypeId>,
    ) -> ParseResult<()> {
        if !seen.insert(type_id) {
            return Ok(());
        }
        let ty = self.get_type(type_id)?.clone();
        let result = match ty {
            Type::DefVal(def) => self.validate_defval_definition(&def, visible, seen),
            Type::Generic(Generic {
                bound: GenericBound::Eq(inner),
                ..
            }) => self.validate_type_ref(inner, visible, seen),
            Type::Generic(Generic {
                bound: GenericBound::Sub,
                ..
            })
            | Type::Resource(_) => Ok(()),
            Type::Func(func_ty) => self.validate_func_definition(&func_ty, visible, seen),
            Type::Component(component_ty) => {
                if self.validated_component_like.borrow().contains(&type_id) {
                    Ok(())
                } else {
                    let result =
                        self.validate_component_surface_inner(&component_ty, visible, seen);
                    if result.is_ok() {
                        self.validated_component_like.borrow_mut().insert(type_id);
                    }
                    result
                }
            }
            Type::Instance(instance_ty) => {
                if self.validated_component_like.borrow().contains(&type_id) {
                    Ok(())
                } else {
                    let result = self.validate_instance_surface_inner(&instance_ty, visible, seen);
                    if result.is_ok() {
                        self.validated_component_like.borrow_mut().insert(type_id);
                    }
                    result
                }
            }
        };
        seen.remove(&type_id);
        result
    }

    fn validate_type_ref(
        &self,
        type_id: TypeId,
        visible: &HashSet<TypeId>,
        seen: &mut HashSet<TypeId>,
    ) -> ParseResult<()> {
        let ty = self.get_type(type_id)?.clone();
        if self.type_requires_name(&ty) {
            if visible.contains(&type_id)
                || self.resource_visible_by_identity(type_id, visible)?
                || self.defval_visible_by_structure(type_id, visible)?
            {
                Ok(())
            } else {
                let ty = self.get_type(type_id)?.clone();
                Err(ComponentParseError::TypeMismatch(format!(
                    "surface type requires exported/imported name: {type_id:?} => {ty:?}"
                )))
            }
        } else {
            self.validate_type_definition(type_id, visible, seen)
        }
    }

    fn resource_visible_by_identity(
        &self,
        type_id: TypeId,
        visible: &HashSet<TypeId>,
    ) -> ParseResult<bool> {
        let Type::Resource(resource) = self.get_type(type_id)? else {
            return Ok(false);
        };

        for candidate in visible {
            if let Type::Resource(candidate_resource) = self.get_type(*candidate)? {
                if candidate_resource == resource {
                    return Ok(true);
                }
            }
        }

        Ok(false)
    }

    fn defval_visible_by_structure(
        &self,
        type_id: TypeId,
        visible: &HashSet<TypeId>,
    ) -> ParseResult<bool> {
        let Type::DefVal(def) = self.get_type(type_id)? else {
            return Ok(false);
        };

        for candidate in visible {
            let Type::DefVal(candidate_def) = self.get_type(*candidate)? else {
                continue;
            };
            if def.assert_subtype_of(candidate_def, self).is_ok()
                && candidate_def.assert_subtype_of(def, self).is_ok()
            {
                return Ok(true);
            }
        }

        Ok(false)
    }

    fn validate_func_definition(
        &self,
        func_ty: &FuncType,
        visible: &HashSet<TypeId>,
        seen: &mut HashSet<TypeId>,
    ) -> ParseResult<()> {
        for param in &func_ty.params {
            self.validate_valtype_ref(param, visible, seen)?;
        }
        if let Some(result) = &func_ty.result {
            self.validate_valtype_ref(result, visible, seen)?;
        }
        Ok(())
    }

    fn validate_valtype_ref(
        &self,
        val_ty: &ValType,
        visible: &HashSet<TypeId>,
        seen: &mut HashSet<TypeId>,
    ) -> ParseResult<()> {
        match val_ty {
            ValType::Primitive(_) => Ok(()),
            ValType::Type(type_id) => self.validate_type_ref(*type_id, visible, seen),
        }
    }

    fn validate_defval_definition(
        &self,
        def: &DefValType,
        visible: &HashSet<TypeId>,
        seen: &mut HashSet<TypeId>,
    ) -> ParseResult<()> {
        match def {
            DefValType::Primitive(_) => Ok(()),
            DefValType::Record(fields) => {
                for field in fields {
                    self.validate_valtype_ref(&field.ty, visible, seen)?;
                }
                Ok(())
            }
            DefValType::Variant(cases) => {
                for case in cases {
                    if let Some(ty) = &case.ty {
                        self.validate_valtype_ref(ty, visible, seen)?;
                    }
                }
                Ok(())
            }
            DefValType::List(ty, _) => self.validate_valtype_ref(ty, visible, seen),
            DefValType::Own(type_id) | DefValType::Borrow(type_id) => {
                self.validate_type_ref(*type_id, visible, seen)
            }
        }
    }

    fn type_requires_name(&self, ty: &Type) -> bool {
        match ty {
            Type::DefVal(def) => self.defval_requires_name(def),
            Type::Generic(Generic {
                bound: GenericBound::Sub,
                ..
            })
            | Type::Resource(_)
            | Type::Func(_)
            | Type::Component(_)
            | Type::Instance(_) => true,
            Type::Generic(Generic {
                bound: GenericBound::Eq(_),
                ..
            }) => false,
        }
    }

    fn defval_requires_name(&self, def: &DefValType) -> bool {
        match def {
            DefValType::Primitive(_) => false,
            DefValType::Record(fields) => !fields
                .iter()
                .enumerate()
                .all(|(index, field)| field.label.0 == index.to_string()),
            DefValType::Variant(cases) => !self.variant_is_inline(cases),
            DefValType::List(_, _) => false,
            DefValType::Own(_) | DefValType::Borrow(_) => false,
        }
    }

    fn collect_component_visible_types(
        &self,
        ty: &ComponentType,
        role: SurfaceRole,
    ) -> ParseResult<HashSet<TypeId>> {
        let mut visible = HashSet::new();
        match role {
            SurfaceRole::Import => {
                for import in ty.imports.values() {
                    if let ComponentImportType::Type { type_id, .. } = import {
                        self.extend_type_visibility(*type_id, &mut visible, &mut HashSet::new())?;
                    }
                }
            }
            SurfaceRole::Export => {
                for export in ty.exports.values() {
                    self.extend_export_visible_types_checked(export, &mut visible)?;
                }
            }
        }
        Ok(visible)
    }

    fn collect_instance_visible_types(&self, ty: &InstanceType) -> ParseResult<HashSet<TypeId>> {
        let mut visible = HashSet::new();
        for export in ty.exports.values() {
            self.extend_instance_export_visible_types(export, &mut visible, &mut HashSet::new())?;
        }
        Ok(visible)
    }

    fn extend_export_visible_types(
        &self,
        export: &ComponentExportType,
        visible: &mut HashSet<TypeId>,
    ) {
        let _ = self.extend_export_visible_types_checked(export, visible);
    }

    fn extend_export_visible_types_checked(
        &self,
        export: &ComponentExportType,
        visible: &mut HashSet<TypeId>,
    ) -> ParseResult<()> {
        match export {
            ComponentExportType::CoreModule(_) => Ok(()),
            ComponentExportType::Component(type_id)
            | ComponentExportType::Instance(type_id)
            | ComponentExportType::Type(type_id)
            | ComponentExportType::Func(type_id) => {
                self.extend_type_visibility(*type_id, visible, &mut HashSet::new())
            }
        }
    }

    fn extend_instance_export_visible_types(
        &self,
        export: &InstanceExportType,
        visible: &mut HashSet<TypeId>,
        visiting: &mut HashSet<TypeId>,
    ) -> ParseResult<()> {
        match export {
            InstanceExportType::CoreModule(_) => Ok(()),
            InstanceExportType::Func(type_id)
            | InstanceExportType::Component(type_id)
            | InstanceExportType::Instance(type_id)
            | InstanceExportType::Type(type_id) => {
                self.extend_type_visibility(*type_id, visible, visiting)
            }
        }
    }

    fn extend_type_visibility(
        &self,
        type_id: TypeId,
        visible: &mut HashSet<TypeId>,
        visiting: &mut HashSet<TypeId>,
    ) -> ParseResult<()> {
        visible.insert(type_id);
        if !visiting.insert(type_id) {
            return Ok(());
        }

        match self.get_type(type_id)? {
            Type::Generic(Generic {
                bound: GenericBound::Eq(inner),
                ..
            }) => {
                self.extend_type_visibility(*inner, visible, visiting)?;
            }
            Type::Component(component_ty) => {
                for import in component_ty.imports.values() {
                    if let ComponentImportType::Type { type_id, .. } = import {
                        self.extend_type_visibility(*type_id, visible, visiting)?;
                    }
                }
                for export in component_ty.exports.values() {
                    self.extend_export_visible_types_checked(export, visible)?;
                }
            }
            Type::Instance(instance_ty) => {
                for export in instance_ty.exports.values() {
                    self.extend_instance_export_visible_types(export, visible, visiting)?;
                }
            }
            Type::DefVal(_) | Type::Func(_) | Type::Resource(_) | Type::Generic(_) => {}
        }
        visiting.remove(&type_id);
        Ok(())
    }

    fn defval_contains_resource_handle(
        &self,
        def: &DefValType,
        visiting: &mut HashSet<TypeId>,
    ) -> ParseResult<bool> {
        match def {
            DefValType::Primitive(_) => Ok(false),
            DefValType::Record(fields) => fields.iter().try_fold(false, |found, field| {
                if found {
                    Ok(true)
                } else {
                    self.valtype_contains_resource_handle(&field.ty, visiting)
                }
            }),
            DefValType::Variant(cases) => cases.iter().try_fold(false, |found, case| {
                if found {
                    Ok(true)
                } else if let Some(ty) = &case.ty {
                    self.valtype_contains_resource_handle(ty, visiting)
                } else {
                    Ok(false)
                }
            }),
            DefValType::List(ty, _) => self.valtype_contains_resource_handle(ty, visiting),
            DefValType::Own(_) | DefValType::Borrow(_) => Ok(true),
        }
    }

    fn valtype_contains_resource_handle(
        &self,
        ty: &ValType,
        visiting: &mut HashSet<TypeId>,
    ) -> ParseResult<bool> {
        match ty {
            ValType::Primitive(_) => Ok(false),
            ValType::Type(type_id) => self.type_contains_resource_handle(*type_id, visiting),
        }
    }

    fn type_contains_resource_handle(
        &self,
        type_id: TypeId,
        visiting: &mut HashSet<TypeId>,
    ) -> ParseResult<bool> {
        if !visiting.insert(type_id) {
            return Ok(false);
        }
        let result = match self.get_type(type_id)? {
            Type::DefVal(def) => self.defval_contains_resource_handle(def, visiting)?,
            Type::Generic(Generic {
                bound: GenericBound::Eq(inner),
                ..
            }) => self.type_contains_resource_handle(*inner, visiting)?,
            Type::Generic(Generic {
                bound: GenericBound::Sub,
                ..
            })
            | Type::Resource(_)
            | Type::Func(_)
            | Type::Component(_)
            | Type::Instance(_) => false,
        };
        visiting.remove(&type_id);
        Ok(result)
    }

    fn type_refs_foreign_resource(
        &self,
        type_id: TypeId,
        owner: ScopeId,
        visiting: &mut HashSet<TypeId>,
    ) -> ParseResult<bool> {
        if !visiting.insert(type_id) {
            return Ok(false);
        }
        let result =
            match self.get_type(type_id)? {
                Type::DefVal(def) => self.defval_refs_foreign_resource(def, owner, visiting)?,
                Type::Generic(Generic {
                    bound: GenericBound::Eq(inner),
                    ..
                }) => self.type_refs_foreign_resource(*inner, owner, visiting)?,
                Type::Generic(Generic {
                    bound: GenericBound::Sub,
                    ..
                }) => false,
                Type::Resource(resource) => resource.owner() != owner,
                Type::Func(func_ty) => {
                    func_ty.params.iter().try_fold(false, |found, param| {
                        if found {
                            Ok(true)
                        } else {
                            self.valtype_refs_foreign_resource(param, owner, visiting)
                        }
                    })? || func_ty.result.as_ref().is_some_and(|result| {
                        self.valtype_refs_foreign_resource(result, owner, visiting)
                            .unwrap_or(false)
                    })
                }
                Type::Component(component_ty) => {
                    let import_foreign =
                        component_ty
                            .imports
                            .values()
                            .try_fold(false, |found, import| {
                                if found {
                                    Ok(true)
                                } else {
                                    match import {
                                        ComponentImportType::CoreModule(_) => Ok(false),
                                        ComponentImportType::Type { type_id, .. } => self
                                            .type_refs_foreign_resource(*type_id, owner, visiting),
                                    }
                                }
                            })?;
                    if import_foreign {
                        true
                    } else {
                        component_ty
                            .exports
                            .values()
                            .try_fold(false, |found, export| {
                                if found {
                                    Ok(true)
                                } else {
                                    match export {
                                        ComponentExportType::CoreModule(_) => Ok(false),
                                        ComponentExportType::Component(type_id)
                                        | ComponentExportType::Instance(type_id)
                                        | ComponentExportType::Type(type_id)
                                        | ComponentExportType::Func(type_id) => self
                                            .type_refs_foreign_resource(*type_id, owner, visiting),
                                    }
                                }
                            })?
                    }
                }
                Type::Instance(instance_ty) => {
                    instance_ty
                        .exports
                        .values()
                        .try_fold(false, |found, export| {
                            if found {
                                Ok(true)
                            } else {
                                match export {
                                    InstanceExportType::CoreModule(_) => Ok(false),
                                    InstanceExportType::Component(type_id)
                                    | InstanceExportType::Instance(type_id)
                                    | InstanceExportType::Type(type_id)
                                    | InstanceExportType::Func(type_id) => {
                                        self.type_refs_foreign_resource(*type_id, owner, visiting)
                                    }
                                }
                            }
                        })?
                }
            };
        visiting.remove(&type_id);
        Ok(result)
    }

    fn defval_refs_foreign_resource(
        &self,
        def: &DefValType,
        owner: ScopeId,
        visiting: &mut HashSet<TypeId>,
    ) -> ParseResult<bool> {
        match def {
            DefValType::Primitive(_) => Ok(false),
            DefValType::Record(fields) => fields.iter().try_fold(false, |found, field| {
                if found {
                    Ok(true)
                } else {
                    self.valtype_refs_foreign_resource(&field.ty, owner, visiting)
                }
            }),
            DefValType::Variant(cases) => cases.iter().try_fold(false, |found, case| {
                if found {
                    Ok(true)
                } else if let Some(ty) = &case.ty {
                    self.valtype_refs_foreign_resource(ty, owner, visiting)
                } else {
                    Ok(false)
                }
            }),
            DefValType::List(ty, _) => self.valtype_refs_foreign_resource(ty, owner, visiting),
            DefValType::Own(type_id) | DefValType::Borrow(type_id) => {
                self.type_refs_foreign_resource(*type_id, owner, visiting)
            }
        }
    }

    fn valtype_refs_foreign_resource(
        &self,
        ty: &ValType,
        owner: ScopeId,
        visiting: &mut HashSet<TypeId>,
    ) -> ParseResult<bool> {
        match ty {
            ValType::Primitive(_) => Ok(false),
            ValType::Type(type_id) => self.type_refs_foreign_resource(*type_id, owner, visiting),
        }
    }

    fn variant_is_inline(&self, cases: &[crate::component::ir::types::Case]) -> bool {
        matches!(
            cases,
            [
                crate::component::ir::types::Case { label, ty: None },
                crate::component::ir::types::Case { label: some, ty: Some(_) }
            ] if label.0 == "none" && some.0 == "some"
        ) || matches!(
            cases,
            [
                crate::component::ir::types::Case { label, .. },
                crate::component::ir::types::Case { label: err, .. }
            ] if label.0 == "ok" && err.0 == "err"
        )
    }

    fn compute_effective_type_size(
        &self,
        type_id: TypeId,
        visiting: &mut HashSet<TypeId>,
    ) -> ParseResult<u64> {
        if let Some(size) = self.type_sizes.borrow().get(&type_id).copied() {
            return Ok(size);
        }
        if !visiting.insert(type_id) {
            return Ok(1);
        }
        let ty = self.get_type(type_id)?.clone();
        let size = self.compute_type_size(&ty, visiting)?;
        visiting.remove(&type_id);
        self.type_sizes.borrow_mut().insert(type_id, size);
        Ok(size)
    }

    fn compute_type_size(&self, ty: &Type, visiting: &mut HashSet<TypeId>) -> ParseResult<u64> {
        match ty {
            Type::DefVal(def) => self.compute_defval_size(def, visiting),
            Type::Generic(Generic {
                bound: GenericBound::Eq(inner),
                ..
            }) => self.compute_effective_type_size(*inner, visiting),
            Type::Generic(Generic {
                bound: GenericBound::Sub,
                ..
            })
            | Type::Resource(_) => Ok(1),
            Type::Func(func_ty) => {
                let mut total = 1;
                for param in &func_ty.params {
                    total = saturating_add(total, self.compute_valtype_size(param, visiting)?);
                    if total >= EFFECTIVE_TYPE_SIZE_CEILING {
                        return Ok(EFFECTIVE_TYPE_SIZE_CEILING);
                    }
                }
                if let Some(result) = &func_ty.result {
                    total = saturating_add(total, self.compute_valtype_size(result, visiting)?);
                    if total >= EFFECTIVE_TYPE_SIZE_CEILING {
                        return Ok(EFFECTIVE_TYPE_SIZE_CEILING);
                    }
                }
                Ok(total)
            }
            Type::Component(component_ty) => {
                let mut total = 1;
                for import in component_ty.imports.values() {
                    total = saturating_add(
                        total,
                        self.compute_component_import_size(import, visiting)?,
                    );
                    if total >= EFFECTIVE_TYPE_SIZE_CEILING {
                        return Ok(EFFECTIVE_TYPE_SIZE_CEILING);
                    }
                }
                for export in component_ty.exports.values() {
                    total = saturating_add(
                        total,
                        self.compute_component_export_size(export, visiting)?,
                    );
                    if total >= EFFECTIVE_TYPE_SIZE_CEILING {
                        return Ok(EFFECTIVE_TYPE_SIZE_CEILING);
                    }
                }
                Ok(total)
            }
            Type::Instance(instance_ty) => {
                let mut total = 1;
                for export in instance_ty.exports.values() {
                    total =
                        saturating_add(total, self.compute_instance_export_size(export, visiting)?);
                    if total >= EFFECTIVE_TYPE_SIZE_CEILING {
                        return Ok(EFFECTIVE_TYPE_SIZE_CEILING);
                    }
                }
                Ok(total)
            }
        }
    }

    fn compute_component_import_size(
        &self,
        import: &ComponentImportType,
        visiting: &mut HashSet<TypeId>,
    ) -> ParseResult<u64> {
        match import {
            ComponentImportType::CoreModule(_) => Ok(1),
            ComponentImportType::Type { type_id, .. } => {
                self.compute_effective_type_size(*type_id, visiting)
            }
        }
    }

    fn compute_component_export_size(
        &self,
        export: &ComponentExportType,
        visiting: &mut HashSet<TypeId>,
    ) -> ParseResult<u64> {
        match export {
            ComponentExportType::CoreModule(_) => Ok(1),
            ComponentExportType::Component(type_id)
            | ComponentExportType::Instance(type_id)
            | ComponentExportType::Type(type_id)
            | ComponentExportType::Func(type_id) => {
                self.compute_effective_type_size(*type_id, visiting)
            }
        }
    }

    fn compute_instance_export_size(
        &self,
        export: &InstanceExportType,
        visiting: &mut HashSet<TypeId>,
    ) -> ParseResult<u64> {
        match export {
            InstanceExportType::CoreModule(_) => Ok(1),
            InstanceExportType::Component(type_id)
            | InstanceExportType::Instance(type_id)
            | InstanceExportType::Type(type_id)
            | InstanceExportType::Func(type_id) => {
                self.compute_effective_type_size(*type_id, visiting)
            }
        }
    }

    fn compute_defval_size(
        &self,
        def: &DefValType,
        visiting: &mut HashSet<TypeId>,
    ) -> ParseResult<u64> {
        match def {
            DefValType::Primitive(_) => Ok(1),
            DefValType::Record(fields) => {
                let mut total = 1;
                for field in fields {
                    total = saturating_add(total, self.compute_valtype_size(&field.ty, visiting)?);
                    if total >= EFFECTIVE_TYPE_SIZE_CEILING {
                        return Ok(EFFECTIVE_TYPE_SIZE_CEILING);
                    }
                }
                Ok(total)
            }
            DefValType::Variant(cases) => {
                let mut total = 1;
                for case in cases {
                    if let Some(ty) = &case.ty {
                        total = saturating_add(total, self.compute_valtype_size(ty, visiting)?);
                    } else {
                        total = saturating_add(total, 1);
                    }
                    if total >= EFFECTIVE_TYPE_SIZE_CEILING {
                        return Ok(EFFECTIVE_TYPE_SIZE_CEILING);
                    }
                }
                Ok(total)
            }
            DefValType::List(ty, maybe_len) => {
                let elem = self.compute_valtype_size(ty, visiting)?;
                Ok(match maybe_len {
                    Some(len) => saturating_mul(elem, *len as u64),
                    None => saturating_add(elem, 1),
                })
            }
            DefValType::Own(type_id) | DefValType::Borrow(type_id) => {
                self.compute_effective_type_size(*type_id, visiting)
            }
        }
    }

    fn compute_valtype_size(
        &self,
        ty: &ValType,
        visiting: &mut HashSet<TypeId>,
    ) -> ParseResult<u64> {
        match ty {
            ValType::Primitive(_) => Ok(1),
            ValType::Type(type_id) => self.compute_effective_type_size(*type_id, visiting),
        }
    }
}

const EFFECTIVE_TYPE_SIZE_CEILING: u64 = 1_000_001;

fn saturating_add(lhs: u64, rhs: u64) -> u64 {
    lhs.saturating_add(rhs).min(EFFECTIVE_TYPE_SIZE_CEILING)
}

fn saturating_mul(lhs: u64, rhs: u64) -> u64 {
    lhs.saturating_mul(rhs).min(EFFECTIVE_TYPE_SIZE_CEILING)
}
