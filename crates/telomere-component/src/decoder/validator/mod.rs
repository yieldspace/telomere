mod arena;
mod scope;
mod size;
mod state;
mod subtyping;
mod transform;
mod validate_types;

use super::ComponentParseError;
use crate::decoder::ParseResult;
use crate::ir::types::{
    ComponentExportType, ComponentImportType, ComponentType, DefValType, FuncType, Generic,
    GenericBound, InstanceExportType, InstanceType, Type, ValType,
};
use crate::ir::{ResourceId, ScopeId, TypeId};
pub(crate) use arena::TransformContext;
use arena::{
    ComponentExportEntry, ComponentImportEntry, ComponentImportKind, ComponentSurfaceSummary,
    InstanceExportEntry, InstanceSurfaceSummary, ResourceOwnerSummary, TypeArena,
    TypeTransformKind, ValidationState,
};
pub use scope::ExportInfo;
pub use scope::ScopeGuard;
pub use state::ParseState;
use std::cell::Cell;
use std::collections::HashMap;
use tracing::trace;
use typed_arena::Arena;

pub struct Validator<'a> {
    arena: &'a Arena<ScopeGuard>,
    scopes: Vec<&'a mut ScopeGuard>,
    scope_kinds: Vec<ScopeKind>,
    types: TypeArena,
    transform_generation: Cell<u32>,
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

fn contains_type_id(sorted: &[TypeId], type_id: TypeId) -> bool {
    sorted.binary_search(&type_id).is_ok()
}

fn merge_type_ids(into: &mut Vec<TypeId>, other: &[TypeId]) {
    if other.is_empty() {
        return;
    }
    if into.is_empty() {
        into.extend_from_slice(other);
        return;
    }

    for type_id in other {
        if let Err(index) = into.binary_search(type_id) {
            into.insert(index, *type_id);
        }
    }
}

impl<'a> Validator<'a> {
    pub fn new(arena: &'a Arena<ScopeGuard>) -> Self {
        let current = arena.alloc(ScopeGuard::new(ScopeId::new(0)));
        Self {
            arena,
            scopes: vec![current],
            scope_kinds: vec![ScopeKind::Concrete],
            types: TypeArena::default(),
            transform_generation: Cell::new(0),
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
        self.types.add(ty)
    }

    pub fn get_type(&self, id: TypeId) -> ParseResult<&Type> {
        self.types
            .get(id)
            .ok_or(ComponentParseError::TypeNotFound(id))
    }

    pub fn snapshot_types(&self) -> Box<[Type]> {
        self.types.snapshot_types()
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
}

#[cfg(test)]
mod tests {
    use super::size::{saturating_add, saturating_mul, VisitTracker, EFFECTIVE_TYPE_SIZE_CEILING};
    use super::*;
    use crate::ir::types::{LabelValType, PrimValType};
    use crate::ir::Label;
    use std::collections::{HashMap, HashSet};

    fn new_validator() -> Validator<'static> {
        let arena = Box::leak(Box::new(Arena::new()));
        Validator::new(arena)
    }

    fn field(label: &str, ty: ValType) -> LabelValType {
        LabelValType::new(Label::new(label), ty)
    }

    fn extract_instance_export_type(
        validator: &Validator<'_>,
        type_id: TypeId,
        name: &str,
    ) -> TypeId {
        let instance = validator.get_instance_type(type_id).unwrap();
        let InstanceExportType::Type(type_id) = instance.exports.get(name).unwrap() else {
            panic!("expected type export");
        };
        *type_id
    }

    fn naive_contains_resource_handle(
        validator: &Validator<'_>,
        type_id: TypeId,
        visiting: &mut HashSet<TypeId>,
    ) -> ParseResult<bool> {
        if !visiting.insert(type_id) {
            return Ok(false);
        }
        let result = match validator.get_type(type_id)? {
            Type::DefVal(def) => match def {
                DefValType::Primitive(_) => false,
                DefValType::Record(fields) => fields.iter().any(|field| {
                    naive_val_contains_resource_handle(validator, &field.ty, visiting).unwrap()
                }),
                DefValType::Variant(cases) => cases.iter().any(|case| {
                    case.ty
                        .as_ref()
                        .map(|ty| {
                            naive_val_contains_resource_handle(validator, ty, visiting).unwrap()
                        })
                        .unwrap_or(false)
                }),
                DefValType::Flags(_) => false,
                DefValType::List(ty, _) => {
                    naive_val_contains_resource_handle(validator, ty, visiting)?
                }
                DefValType::Own(_) | DefValType::Borrow(_) => true,
            },
            Type::Generic(Generic {
                bound: GenericBound::Eq(inner),
                ..
            }) => naive_contains_resource_handle(validator, *inner, visiting)?,
            Type::Generic(Generic {
                bound: GenericBound::Sub,
                ..
            })
            | Type::Resource(_)
            | Type::Func(_)
            | Type::Component(_)
            | Type::Instance(_) => false,
        };
        let _ = visiting.remove(&type_id);
        Ok(result)
    }

    fn naive_val_contains_resource_handle(
        validator: &Validator<'_>,
        val: &ValType,
        visiting: &mut HashSet<TypeId>,
    ) -> ParseResult<bool> {
        match val {
            ValType::Primitive(_) => Ok(false),
            ValType::Type(type_id) => naive_contains_resource_handle(validator, *type_id, visiting),
        }
    }

    fn naive_refs_foreign_resource(
        validator: &Validator<'_>,
        type_id: TypeId,
        owner: ScopeId,
        visiting: &mut HashSet<TypeId>,
    ) -> ParseResult<bool> {
        if !visiting.insert(type_id) {
            return Ok(false);
        }
        let result = match validator.get_type(type_id)? {
            Type::DefVal(def) => match def {
                DefValType::Primitive(_) => false,
                DefValType::Record(fields) => fields.iter().any(|field| {
                    naive_val_refs_foreign_resource(validator, &field.ty, owner, visiting).unwrap()
                }),
                DefValType::Variant(cases) => cases.iter().any(|case| {
                    case.ty
                        .as_ref()
                        .map(|ty| {
                            naive_val_refs_foreign_resource(validator, ty, owner, visiting).unwrap()
                        })
                        .unwrap_or(false)
                }),
                DefValType::Flags(_) => false,
                DefValType::List(ty, _) => {
                    naive_val_refs_foreign_resource(validator, ty, owner, visiting)?
                }
                DefValType::Own(type_id) | DefValType::Borrow(type_id) => {
                    naive_refs_foreign_resource(validator, *type_id, owner, visiting)?
                }
            },
            Type::Generic(Generic {
                bound: GenericBound::Eq(inner),
                ..
            }) => naive_refs_foreign_resource(validator, *inner, owner, visiting)?,
            Type::Generic(Generic {
                bound: GenericBound::Sub,
                ..
            }) => false,
            Type::Resource(resource) => resource.owner() != owner,
            Type::Func(func_ty) => {
                func_ty.params.iter().any(|param| {
                    naive_val_refs_foreign_resource(validator, param, owner, visiting).unwrap()
                }) || func_ty.result.as_ref().is_some_and(|result| {
                    naive_val_refs_foreign_resource(validator, result, owner, visiting).unwrap()
                })
            }
            Type::Component(component_ty) => {
                component_ty.imports.values().any(|import| match import {
                    ComponentImportType::CoreModule(_) => false,
                    ComponentImportType::Type { type_id, .. } => {
                        naive_refs_foreign_resource(validator, *type_id, owner, visiting).unwrap()
                    }
                }) || component_ty.exports.values().any(|export| match export {
                    ComponentExportType::CoreModule(_) => false,
                    ComponentExportType::Component(type_id)
                    | ComponentExportType::Instance(type_id)
                    | ComponentExportType::Type(type_id)
                    | ComponentExportType::Func(type_id) => {
                        naive_refs_foreign_resource(validator, *type_id, owner, visiting).unwrap()
                    }
                })
            }
            Type::Instance(instance_ty) => {
                instance_ty.exports.values().any(|export| match export {
                    InstanceExportType::CoreModule(_) => false,
                    InstanceExportType::Component(type_id)
                    | InstanceExportType::Instance(type_id)
                    | InstanceExportType::Type(type_id)
                    | InstanceExportType::Func(type_id) => {
                        naive_refs_foreign_resource(validator, *type_id, owner, visiting).unwrap()
                    }
                })
            }
        };
        let _ = visiting.remove(&type_id);
        Ok(result)
    }

    fn naive_val_refs_foreign_resource(
        validator: &Validator<'_>,
        val: &ValType,
        owner: ScopeId,
        visiting: &mut HashSet<TypeId>,
    ) -> ParseResult<bool> {
        match val {
            ValType::Primitive(_) => Ok(false),
            ValType::Type(type_id) => {
                naive_refs_foreign_resource(validator, *type_id, owner, visiting)
            }
        }
    }

    fn naive_effective_type_size(
        validator: &Validator<'_>,
        type_id: TypeId,
        visiting: &mut HashSet<TypeId>,
    ) -> ParseResult<u64> {
        if !visiting.insert(type_id) {
            return Ok(1);
        }
        let ty = validator.get_type(type_id)?.clone();
        let result = match ty {
            Type::DefVal(def) => naive_defval_size(validator, &def, visiting)?,
            Type::Generic(Generic {
                bound: GenericBound::Eq(inner),
                ..
            }) => naive_effective_type_size(validator, inner, visiting)?,
            Type::Generic(Generic {
                bound: GenericBound::Sub,
                ..
            })
            | Type::Resource(_) => 1,
            Type::Func(func_ty) => {
                let mut total = 1;
                for param in &func_ty.params {
                    total = saturating_add(total, naive_val_size(validator, param, visiting)?);
                }
                if let Some(result) = &func_ty.result {
                    total = saturating_add(total, naive_val_size(validator, result, visiting)?);
                }
                total
            }
            Type::Component(component_ty) => {
                let mut total = 1;
                for import in component_ty.imports.values() {
                    total = saturating_add(
                        total,
                        match import {
                            ComponentImportType::CoreModule(_) => 1,
                            ComponentImportType::Type { type_id, .. } => {
                                naive_effective_type_size(validator, *type_id, visiting)?
                            }
                        },
                    );
                }
                for export in component_ty.exports.values() {
                    total = saturating_add(
                        total,
                        match export {
                            ComponentExportType::CoreModule(_) => 1,
                            ComponentExportType::Component(type_id)
                            | ComponentExportType::Instance(type_id)
                            | ComponentExportType::Type(type_id)
                            | ComponentExportType::Func(type_id) => {
                                naive_effective_type_size(validator, *type_id, visiting)?
                            }
                        },
                    );
                }
                total
            }
            Type::Instance(instance_ty) => {
                let mut total = 1;
                for export in instance_ty.exports.values() {
                    total = saturating_add(
                        total,
                        match export {
                            InstanceExportType::CoreModule(_) => 1,
                            InstanceExportType::Component(type_id)
                            | InstanceExportType::Instance(type_id)
                            | InstanceExportType::Type(type_id)
                            | InstanceExportType::Func(type_id) => {
                                naive_effective_type_size(validator, *type_id, visiting)?
                            }
                        },
                    );
                }
                total
            }
        };
        let _ = visiting.remove(&type_id);
        Ok(result.min(EFFECTIVE_TYPE_SIZE_CEILING))
    }

    fn naive_defval_size(
        validator: &Validator<'_>,
        def: &DefValType,
        visiting: &mut HashSet<TypeId>,
    ) -> ParseResult<u64> {
        match def {
            DefValType::Primitive(_) => Ok(1),
            DefValType::Record(fields) => {
                let mut total = 1;
                for field in fields {
                    total = saturating_add(total, naive_val_size(validator, &field.ty, visiting)?);
                }
                Ok(total)
            }
            DefValType::Variant(cases) => {
                let mut total = 1;
                for case in cases {
                    total = saturating_add(
                        total,
                        if let Some(ty) = &case.ty {
                            naive_val_size(validator, ty, visiting)?
                        } else {
                            1
                        },
                    );
                }
                Ok(total)
            }
            DefValType::Flags(labels) => Ok((labels.len() as u64).div_ceil(32).max(1)),
            DefValType::List(ty, maybe_len) => {
                let elem = naive_val_size(validator, ty, visiting)?;
                Ok(match maybe_len {
                    Some(len) => saturating_mul(elem, *len as u64),
                    None => saturating_add(elem, 1),
                })
            }
            DefValType::Own(type_id) | DefValType::Borrow(type_id) => {
                naive_effective_type_size(validator, *type_id, visiting)
            }
        }
    }

    fn naive_val_size(
        validator: &Validator<'_>,
        ty: &ValType,
        visiting: &mut HashSet<TypeId>,
    ) -> ParseResult<u64> {
        match ty {
            ValType::Primitive(_) => Ok(1),
            ValType::Type(type_id) => naive_effective_type_size(validator, *type_id, visiting),
        }
    }

    #[test]
    fn component_surface_subtype_matches_hashmap_version() {
        let mut validator = new_validator();
        let scalar = validator.new_type(Type::DefVal(DefValType::Primitive(PrimValType::U32)));
        let func = validator.new_type(Type::Func(FuncType {
            params: vec![ValType::Type(scalar)],
            param_names: vec![Label::new("x")],
            result: Some(ValType::Type(scalar)),
        }));

        let parent = validator.new_type(Type::Component(ComponentType {
            import_order: vec!["dep".to_owned()],
            imports: HashMap::from([(
                "dep".to_owned(),
                ComponentImportType::Type {
                    type_id: scalar,
                    generic: Generic::new(GenericBound::Eq(scalar)),
                },
            )]),
            exports: HashMap::from([("run".to_owned(), ComponentExportType::Func(func))]),
            generics_replacing_program: Vec::new(),
        }));

        let child = validator.new_type(Type::Component(ComponentType {
            import_order: vec!["dep".to_owned()],
            imports: HashMap::from([(
                "dep".to_owned(),
                ComponentImportType::Type {
                    type_id: scalar,
                    generic: Generic::new(GenericBound::Eq(scalar)),
                },
            )]),
            exports: HashMap::from([
                ("z-extra".to_owned(), ComponentExportType::Type(scalar)),
                ("run".to_owned(), ComponentExportType::Func(func)),
            ]),
            generics_replacing_program: Vec::new(),
        }));

        let dense = validator.assert_component_type_ids_subtype_of(child, parent);
        let map = validator
            .get_component_type(child)
            .unwrap()
            .assert_subtype_of(validator.get_component_type(parent).unwrap(), &validator);
        assert_eq!(dense.is_ok(), map.is_ok());

        let mismatch = validator.new_type(Type::Component(ComponentType {
            import_order: vec!["other".to_owned()],
            imports: HashMap::from([(
                "other".to_owned(),
                ComponentImportType::Type {
                    type_id: scalar,
                    generic: Generic::new(GenericBound::Eq(scalar)),
                },
            )]),
            exports: HashMap::from([("run".to_owned(), ComponentExportType::Func(func))]),
            generics_replacing_program: Vec::new(),
        }));

        let dense_error = validator
            .assert_component_type_ids_subtype_of(mismatch, parent)
            .unwrap_err()
            .to_string();
        let map_error = validator
            .get_component_type(mismatch)
            .unwrap()
            .assert_subtype_of(validator.get_component_type(parent).unwrap(), &validator)
            .unwrap_err()
            .to_string();
        assert_eq!(dense_error, map_error);
    }

    #[test]
    fn instance_surface_subtype_matches_hashmap_version() {
        let mut validator = new_validator();
        let scalar = validator.new_type(Type::DefVal(DefValType::Primitive(PrimValType::Bool)));
        let func = validator.new_type(Type::Func(FuncType {
            params: vec![ValType::Type(scalar)],
            param_names: vec![Label::new("flag")],
            result: Some(ValType::Type(scalar)),
        }));

        let parent = validator.new_type(Type::Instance(InstanceType {
            exports: HashMap::from([("run".to_owned(), InstanceExportType::Func(func))]),
        }));
        let child = validator.new_type(Type::Instance(InstanceType {
            exports: HashMap::from([
                ("z-extra".to_owned(), InstanceExportType::Type(scalar)),
                ("run".to_owned(), InstanceExportType::Func(func)),
            ]),
        }));

        let dense = validator.assert_instance_type_ids_subtype_of(child, parent);
        let map = validator
            .get_instance_type(child)
            .unwrap()
            .assert_subtype_of(validator.get_instance_type(parent).unwrap(), &validator);
        assert_eq!(dense.is_ok(), map.is_ok());
    }

    #[test]
    fn type_arena_keeps_dense_indices_stable() {
        let mut validator = new_validator();
        let first = validator.new_type(Type::DefVal(DefValType::Primitive(PrimValType::Bool)));
        let second = validator.new_type(Type::DefVal(DefValType::Primitive(PrimValType::S32)));

        assert_eq!(first.index(), 0);
        assert_eq!(second.index(), 1);
        assert!(matches!(
            validator.get_type(first).unwrap(),
            Type::DefVal(DefValType::Primitive(PrimValType::Bool))
        ));
        assert!(matches!(
            validator.get_type(second).unwrap(),
            Type::DefVal(DefValType::Primitive(PrimValType::S32))
        ));
    }

    #[test]
    fn freshen_import_keeps_top_level_resources_unique_and_reuses_session_cache() {
        let mut validator = new_validator();
        let sub = validator.new_type(Type::Generic(Generic::new(GenericBound::Sub)));
        let template = validator.new_type(Type::Instance(InstanceType {
            exports: HashMap::from([("r".to_owned(), InstanceExportType::Type(sub))]),
        }));

        let first = validator.freshen_import_type_id(template).unwrap();
        let second = validator.freshen_import_type_id(template).unwrap();
        assert_ne!(
            extract_instance_export_type(&validator, first, "r"),
            extract_instance_export_type(&validator, second, "r")
        );

        let repeated = validator.new_type(Type::DefVal(DefValType::Record(vec![
            field("0", ValType::Type(sub)),
            field("1", ValType::Type(sub)),
        ])));
        let mut unified = validator.new_transform_context();
        let _ = validator
            .instantiate_type_id(repeated, &mut unified)
            .unwrap();
        assert!(validator.types.transform_cache_len() > 0);
    }

    #[test]
    fn memoized_summaries_match_recursive_walk() {
        let mut validator = new_validator();
        let local_owner = validator.current_scope_id();
        let foreign_owner = ScopeId::new(99);
        let local_resource = validator.new_type(Type::Resource(ResourceId::new(local_owner)));
        let foreign_resource = validator.new_type(Type::Resource(ResourceId::new(foreign_owner)));
        let local_handle = validator.new_type(Type::DefVal(DefValType::Own(local_resource)));
        let foreign_list = validator.new_type(Type::DefVal(DefValType::List(
            ValType::Type(foreign_resource),
            Some(2),
        )));
        let record = validator.new_type(Type::DefVal(DefValType::Record(vec![
            field("lhs", ValType::Type(local_handle)),
            field("rhs", ValType::Type(foreign_list)),
        ])));

        let mut size_seen = HashSet::new();
        let expected_size = naive_effective_type_size(&validator, record, &mut size_seen).unwrap();
        let mut size_visiting = VisitTracker::new(validator.types.len());
        let actual_size = validator
            .compute_effective_type_size(record, &mut size_visiting)
            .unwrap();
        assert_eq!(actual_size, expected_size);

        let mut handle_seen = HashSet::new();
        let expected_contains =
            naive_contains_resource_handle(&validator, record, &mut handle_seen).unwrap();
        let actual_contains = validator.contains_resource_handle(record).unwrap();
        assert_eq!(actual_contains, expected_contains);

        let mut foreign_seen = HashSet::new();
        let expected_foreign =
            naive_refs_foreign_resource(&validator, record, local_owner, &mut foreign_seen)
                .unwrap();
        let mut owner_visiting = VisitTracker::new(validator.types.len());
        let actual_foreign = validator
            .resource_owner_summary(record, &mut owner_visiting)
            .unwrap()
            .refs_foreign_resource(local_owner);
        assert_eq!(actual_foreign, expected_foreign);
    }

    #[test]
    fn visible_closure_handles_mutually_recursive_component_instance_exports() {
        let mut validator = new_validator();
        let component_id = validator.new_type(Type::Component(ComponentType {
            import_order: Vec::new(),
            imports: HashMap::new(),
            exports: HashMap::from([(
                "inst".to_owned(),
                ComponentExportType::Instance(TypeId::from_index(1)),
            )]),
            generics_replacing_program: Vec::new(),
        }));
        let instance_id = validator.new_type(Type::Instance(InstanceType {
            exports: HashMap::from([(
                "comp".to_owned(),
                InstanceExportType::Component(TypeId::from_index(0)),
            )]),
        }));

        assert_eq!(component_id, TypeId::from_index(0));
        assert_eq!(instance_id, TypeId::from_index(1));

        let closure = validator.visible_closure(component_id).unwrap();
        assert_eq!(closure, vec![component_id, instance_id]);
        validator
            .validate_component_type_definition(component_id)
            .unwrap();
    }

    #[test]
    fn recursive_defval_surface_does_not_recurse_in_handle_check() {
        let mut validator = new_validator();
        let record_id = validator.new_type(Type::DefVal(DefValType::Record(vec![field(
            "0",
            ValType::Type(TypeId::from_index(0)),
        )])));

        assert_eq!(record_id, TypeId::from_index(0));
        assert!(!validator.contains_resource_handle(record_id).unwrap());
        assert!(!validator.contains_resource_handle(record_id).unwrap());

        let component_id = validator.new_type(Type::Component(ComponentType {
            import_order: Vec::new(),
            imports: HashMap::new(),
            exports: HashMap::from([("node".to_owned(), ComponentExportType::Type(record_id))]),
            generics_replacing_program: Vec::new(),
        }));

        validator
            .validate_component_type_definition(component_id)
            .unwrap();
    }

    #[test]
    fn surface_summary_interns_repeated_names() {
        let mut validator = new_validator();
        let scalar = validator.new_type(Type::DefVal(DefValType::Primitive(PrimValType::String)));

        let left = validator.new_type(Type::Component(ComponentType {
            import_order: vec!["alpha".to_owned()],
            imports: HashMap::from([(
                "alpha".to_owned(),
                ComponentImportType::Type {
                    type_id: scalar,
                    generic: Generic::new(GenericBound::Eq(scalar)),
                },
            )]),
            exports: HashMap::from([("omega".to_owned(), ComponentExportType::Type(scalar))]),
            generics_replacing_program: Vec::new(),
        }));
        let right = validator.new_type(Type::Component(ComponentType {
            import_order: vec!["alpha".to_owned()],
            imports: HashMap::from([(
                "alpha".to_owned(),
                ComponentImportType::Type {
                    type_id: scalar,
                    generic: Generic::new(GenericBound::Eq(scalar)),
                },
            )]),
            exports: HashMap::from([("omega".to_owned(), ComponentExportType::Type(scalar))]),
            generics_replacing_program: Vec::new(),
        }));

        let left_summary = validator.component_surface_summary(left).unwrap();
        let right_summary = validator.component_surface_summary(right).unwrap();
        assert_eq!(left_summary.imports[0].name, right_summary.imports[0].name);
        assert_eq!(left_summary.exports[0].name, right_summary.exports[0].name);
        assert_eq!(validator.types.interned_name_count(), 2);
    }
}
