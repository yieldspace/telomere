mod arena;
mod scope;
mod state;

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

struct VisitTracker {
    epoch: u32,
    marks: Vec<u32>,
}

impl VisitTracker {
    fn new(type_count: usize) -> Self {
        Self {
            epoch: 1,
            marks: vec![0; type_count],
        }
    }

    fn enter(&mut self, type_id: TypeId) -> bool {
        let slot = &mut self.marks[type_id.index() as usize];
        if *slot == self.epoch {
            return false;
        }
        *slot = self.epoch;
        true
    }

    fn leave(&mut self, type_id: TypeId) {
        self.marks[type_id.index() as usize] = 0;
    }
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

    pub fn assert_type_ids_subtype_of(&self, child: TypeId, parent: TypeId) -> ParseResult<()> {
        if child == parent {
            return Ok(());
        }

        match (self.get_type(child)?, self.get_type(parent)?) {
            (Type::Component(_), Type::Component(_)) => {
                self.assert_component_type_ids_subtype_of(child, parent)
            }
            (Type::Instance(_), Type::Instance(_)) => {
                self.assert_instance_type_ids_subtype_of(child, parent)
            }
            (lhs, rhs) => lhs.assert_subtype_of(rhs, self),
        }
    }

    pub fn assert_component_type_ids_subtype_of(
        &self,
        child: TypeId,
        parent: TypeId,
    ) -> ParseResult<()> {
        let child = self.component_surface_summary(child)?;
        let parent = self.component_surface_summary(parent)?;

        if child.imports.len() > parent.imports.len() {
            Err(ComponentParseError::TypeMismatch(
                "import count mismatch".to_owned(),
            ))?
        }

        let mut parent_index = 0;
        for child_import in &child.imports {
            while let Some(parent_import) = parent.imports.get(parent_index) {
                if parent_import.name < child_import.name {
                    parent_index += 1;
                    continue;
                }
                break;
            }
            let Some(parent_import) = parent.imports.get(parent_index) else {
                Err(ComponentParseError::TypeMismatch(
                    "import name mismatch".to_owned(),
                ))?
            };
            if parent_import.name != child_import.name {
                Err(ComponentParseError::TypeMismatch(
                    "import name mismatch".to_owned(),
                ))?
            }
            match (&child_import.kind, &parent_import.kind) {
                (
                    ComponentImportKind::Type(ComponentImportType::Type { generic: child, .. }),
                    ComponentImportKind::Type(ComponentImportType::Type {
                        generic: parent, ..
                    }),
                ) => child.bound.assert_subtype_of(&parent.bound, self)?,
                (
                    ComponentImportKind::CoreModule(child),
                    ComponentImportKind::CoreModule(parent),
                ) if child == parent => {}
                _ => Err(ComponentParseError::TypeMismatch(
                    "import kind mismatch".to_owned(),
                ))?,
            }
        }

        if parent.exports.len() > child.exports.len() {
            Err(ComponentParseError::TypeMismatch(
                "export count mismatch".to_owned(),
            ))?
        }

        let mut child_index = 0;
        for parent_export in &parent.exports {
            while let Some(child_export) = child.exports.get(child_index) {
                if child_export.name < parent_export.name {
                    child_index += 1;
                    continue;
                }
                break;
            }
            let Some(child_export) = child.exports.get(child_index) else {
                Err(ComponentParseError::TypeMismatch(
                    "import name mismatch".to_owned(),
                ))?
            };
            if child_export.name != parent_export.name {
                Err(ComponentParseError::TypeMismatch(
                    "import name mismatch".to_owned(),
                ))?
            }
            parent_export
                .kind
                .assert_subtype_of(&child_export.kind, self)?;
        }

        Ok(())
    }

    pub fn assert_instance_type_ids_subtype_of(
        &self,
        child: TypeId,
        parent: TypeId,
    ) -> ParseResult<()> {
        let child = self.instance_surface_summary(child)?;
        let parent = self.instance_surface_summary(parent)?;

        if child.exports.len() < parent.exports.len() {
            Err(ComponentParseError::TypeMismatch(
                "instance export count".to_owned(),
            ))?
        }

        let mut child_index = 0;
        for parent_export in &parent.exports {
            while let Some(child_export) = child.exports.get(child_index) {
                if child_export.name < parent_export.name {
                    child_index += 1;
                    continue;
                }
                break;
            }
            let Some(child_export) = child.exports.get(child_index) else {
                Err(ComponentParseError::TypeMismatch(
                    "instance export mismatch".to_owned(),
                ))?
            };
            if child_export.name != parent_export.name {
                Err(ComponentParseError::TypeMismatch(
                    "instance export mismatch".to_owned(),
                ))?
            }
            child_export
                .kind
                .assert_subtype_of(&parent_export.kind, self)?;
        }

        Ok(())
    }

    fn next_transform_generation(&self) -> u32 {
        let next = self.transform_generation.get().wrapping_add(1).max(1);
        self.transform_generation.set(next);
        next
    }

    pub(crate) fn new_transform_context(&self) -> TransformContext {
        TransformContext::new(self.next_transform_generation())
    }

    fn component_surface_summary(
        &self,
        type_id: TypeId,
    ) -> ParseResult<std::cell::Ref<'_, ComponentSurfaceSummary>> {
        if self.types.component_surface_summary(type_id).is_none() {
            let ty = self.get_component_type(type_id)?.clone();
            let mut imports = ty
                .imports
                .iter()
                .map(|(name, import)| ComponentImportEntry {
                    name: self.types.intern_name(name),
                    kind: match import {
                        ComponentImportType::CoreModule(module) => {
                            ComponentImportKind::CoreModule(module.clone())
                        }
                        _ => ComponentImportKind::Type(import.clone()),
                    },
                })
                .collect::<Vec<_>>();
            imports.sort_unstable_by_key(|entry| entry.name);

            let mut exports = ty
                .exports
                .iter()
                .map(|(name, export)| ComponentExportEntry {
                    name: self.types.intern_name(name),
                    kind: export.clone(),
                })
                .collect::<Vec<_>>();
            exports.sort_unstable_by_key(|entry| entry.name);

            self.types.set_component_surface_summary(
                type_id,
                ComponentSurfaceSummary {
                    imports: imports.into_boxed_slice(),
                    exports: exports.into_boxed_slice(),
                },
            );
        }

        self.types
            .component_surface_summary(type_id)
            .ok_or_else(|| {
                ComponentParseError::TypeMismatch("component surface summary is missing".to_owned())
            })
    }

    fn instance_surface_summary(
        &self,
        type_id: TypeId,
    ) -> ParseResult<std::cell::Ref<'_, InstanceSurfaceSummary>> {
        if self.types.instance_surface_summary(type_id).is_none() {
            let ty = self.get_instance_type(type_id)?.clone();
            let mut exports = ty
                .exports
                .iter()
                .map(|(name, export)| InstanceExportEntry {
                    name: self.types.intern_name(name),
                    kind: export.clone(),
                })
                .collect::<Vec<_>>();
            exports.sort_unstable_by_key(|entry| entry.name);

            self.types.set_instance_surface_summary(
                type_id,
                InstanceSurfaceSummary {
                    exports: exports.into_boxed_slice(),
                },
            );
        }

        self.types.instance_surface_summary(type_id).ok_or_else(|| {
            ComponentParseError::TypeMismatch("instance surface summary is missing".to_owned())
        })
    }

    pub(crate) fn instantiate_type_id(
        &mut self,
        type_id: TypeId,
        context: &mut TransformContext,
    ) -> ParseResult<TypeId> {
        if let Some(mapped) = context.get(type_id) {
            return Ok(mapped);
        }
        let env_id = self.types.subst_env_id(context);
        if let Some(mapped) = self.types.lookup_transform(
            type_id,
            TypeTransformKind::Instantiate,
            context.generation(),
            env_id,
        ) {
            context.insert(type_id, mapped);
            return Ok(mapped);
        }

        let ty = self.get_type(type_id)?.clone();
        let cloned = self.instantiate_type(&ty, context)?;
        let new_type_id = self.new_type(cloned);
        self.validate_effective_type_size(new_type_id)?;
        self.types.record_transform(
            type_id,
            TypeTransformKind::Instantiate,
            context.generation(),
            env_id,
            new_type_id,
        );
        context.insert(type_id, new_type_id);
        Ok(new_type_id)
    }

    pub(crate) fn instantiate_sub_resource_type(
        &mut self,
        type_id: TypeId,
        context: &mut TransformContext,
    ) -> ParseResult<TypeId> {
        if let Some(mapped) = context.get(type_id) {
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

        let env_id = self.types.subst_env_id(context);
        if let Some(mapped) = self.types.lookup_transform(
            type_id,
            TypeTransformKind::SubResource,
            context.generation(),
            env_id,
        ) {
            context.insert(type_id, mapped);
            return Ok(mapped);
        }

        let new_type_id = self.new_type(Type::Resource(ResourceId::synthetic()));
        self.validate_effective_type_size(new_type_id)?;
        self.types.record_transform(
            type_id,
            TypeTransformKind::SubResource,
            context.generation(),
            env_id,
            new_type_id,
        );
        context.insert(type_id, new_type_id);
        Ok(new_type_id)
    }

    pub fn freshen_import_type_id(&mut self, type_id: TypeId) -> ParseResult<TypeId> {
        let mut context = self.new_transform_context();
        self.freshen_import_type_id_with_context(type_id, &mut context)
    }

    pub(crate) fn resolve_surface_type_id(
        &mut self,
        type_id: TypeId,
        context: &mut TransformContext,
    ) -> ParseResult<TypeId> {
        self.freshen_import_type_id_with_context(type_id, context)
    }

    fn freshen_import_type_id_with_context(
        &mut self,
        type_id: TypeId,
        context: &mut TransformContext,
    ) -> ParseResult<TypeId> {
        if let Some(mapped) = context.get(type_id) {
            return Ok(mapped);
        }
        let env_id = self.types.subst_env_id(context);
        if let Some(mapped) = self.types.lookup_transform(
            type_id,
            TypeTransformKind::FreshenImport,
            context.generation(),
            env_id,
        ) {
            context.insert(type_id, mapped);
            return Ok(mapped);
        }

        let ty = self.get_type(type_id)?.clone();
        let cloned = self.freshen_import_type(&ty, context)?;
        let new_type_id = self.new_type(cloned);
        self.validate_effective_type_size(new_type_id)?;
        self.types.record_transform(
            type_id,
            TypeTransformKind::FreshenImport,
            context.generation(),
            env_id,
            new_type_id,
        );
        context.insert(type_id, new_type_id);
        Ok(new_type_id)
    }

    fn instantiate_type(&mut self, ty: &Type, context: &mut TransformContext) -> ParseResult<Type> {
        Ok(match ty {
            Type::DefVal(def) => Type::DefVal(self.instantiate_defval(def, context)?),
            Type::Generic(Generic {
                bound: GenericBound::Eq(inner),
                ..
            }) => {
                let inner = self.instantiate_type_id(*inner, context)?;
                Type::Generic(Generic::new(GenericBound::Eq(inner)))
            }
            Type::Generic(Generic {
                bound: GenericBound::Sub,
                ..
            }) => Type::Generic(Generic::new(GenericBound::Sub)),
            Type::Func(func_ty) => Type::Func(self.instantiate_func(func_ty, context)?),
            Type::Resource(_) => Type::Resource(ResourceId::synthetic()),
            Type::Component(component_ty) => {
                Type::Component(self.instantiate_component_type(component_ty, context)?)
            }
            Type::Instance(instance_ty) => {
                Type::Instance(self.instantiate_instance_type(instance_ty, context)?)
            }
        })
    }

    fn freshen_import_type(
        &mut self,
        ty: &Type,
        context: &mut TransformContext,
    ) -> ParseResult<Type> {
        Ok(match ty {
            Type::DefVal(def) => Type::DefVal(self.freshen_import_defval(def, context)?),
            Type::Generic(Generic {
                bound: GenericBound::Eq(inner),
                ..
            }) => Type::Generic(Generic::new(GenericBound::Eq(
                self.freshen_import_type_id_with_context(*inner, context)?,
            ))),
            Type::Generic(Generic {
                bound: GenericBound::Sub,
                ..
            }) => Type::Generic(Generic::new(GenericBound::Sub)),
            Type::Func(func_ty) => Type::Func(self.freshen_import_func(func_ty, context)?),
            Type::Resource(resource) => Type::Resource(*resource),
            Type::Component(component_ty) => {
                Type::Component(self.freshen_import_component_type(component_ty, context)?)
            }
            Type::Instance(instance_ty) => {
                Type::Instance(self.freshen_import_instance_type(instance_ty, context)?)
            }
        })
    }

    fn instantiate_component_type(
        &mut self,
        ty: &ComponentType,
        context: &mut TransformContext,
    ) -> ParseResult<ComponentType> {
        let mut imports = HashMap::new();
        for (name, import) in &ty.imports {
            let import = match import {
                ComponentImportType::CoreModule(module_ty) => {
                    ComponentImportType::CoreModule(module_ty.clone())
                }
                ComponentImportType::Type { type_id, generic } => {
                    let type_id = self.instantiate_type_id(*type_id, context)?;
                    let generic = match &generic.bound {
                        GenericBound::Eq(inner) => Generic::new(GenericBound::Eq(
                            self.instantiate_type_id(*inner, context)?,
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
                    ComponentExportType::Component(self.instantiate_type_id(*type_id, context)?)
                }
                ComponentExportType::Instance(type_id) => {
                    ComponentExportType::Instance(self.instantiate_type_id(*type_id, context)?)
                }
                ComponentExportType::Type(type_id) => {
                    ComponentExportType::Type(self.instantiate_type_id(*type_id, context)?)
                }
                ComponentExportType::Func(type_id) => {
                    ComponentExportType::Func(self.instantiate_type_id(*type_id, context)?)
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
        context: &mut TransformContext,
    ) -> ParseResult<ComponentType> {
        let mut imports = HashMap::new();
        for (name, import) in &ty.imports {
            let import = match import {
                ComponentImportType::CoreModule(module_ty) => {
                    ComponentImportType::CoreModule(module_ty.clone())
                }
                ComponentImportType::Type { type_id, generic } => {
                    let type_id = self.freshen_import_type_id_with_context(*type_id, context)?;
                    let generic = match &generic.bound {
                        GenericBound::Eq(inner) => Generic::new(GenericBound::Eq(
                            self.freshen_import_type_id_with_context(*inner, context)?,
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
                    self.freshen_import_type_id_with_context(*type_id, context)?,
                ),
                ComponentExportType::Instance(type_id) => ComponentExportType::Instance(
                    self.freshen_import_type_id_with_context(*type_id, context)?,
                ),
                ComponentExportType::Type(type_id) => ComponentExportType::Type(
                    self.freshen_import_type_id_with_context(*type_id, context)?,
                ),
                ComponentExportType::Func(type_id) => ComponentExportType::Func(
                    self.freshen_import_type_id_with_context(*type_id, context)?,
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
        context: &mut TransformContext,
    ) -> ParseResult<InstanceType> {
        let mut exports = HashMap::new();
        for (name, export) in &ty.exports {
            let export = match export {
                InstanceExportType::CoreModule(module_ty) => {
                    InstanceExportType::CoreModule(module_ty.clone())
                }
                InstanceExportType::Func(type_id) => {
                    InstanceExportType::Func(self.instantiate_type_id(*type_id, context)?)
                }
                InstanceExportType::Component(type_id) => {
                    InstanceExportType::Component(self.instantiate_type_id(*type_id, context)?)
                }
                InstanceExportType::Instance(type_id) => {
                    InstanceExportType::Instance(self.instantiate_type_id(*type_id, context)?)
                }
                InstanceExportType::Type(type_id) => {
                    InstanceExportType::Type(self.instantiate_type_id(*type_id, context)?)
                }
            };
            exports.insert(name.clone(), export);
        }
        Ok(InstanceType { exports })
    }

    fn freshen_import_instance_type(
        &mut self,
        ty: &InstanceType,
        context: &mut TransformContext,
    ) -> ParseResult<InstanceType> {
        let mut exports = HashMap::new();
        for (name, export) in &ty.exports {
            let export = match export {
                InstanceExportType::CoreModule(module_ty) => {
                    InstanceExportType::CoreModule(module_ty.clone())
                }
                InstanceExportType::Func(type_id) => InstanceExportType::Func(
                    self.freshen_import_type_id_with_context(*type_id, context)?,
                ),
                InstanceExportType::Component(type_id) => InstanceExportType::Component(
                    self.freshen_import_type_id_with_context(*type_id, context)?,
                ),
                InstanceExportType::Instance(type_id) => InstanceExportType::Instance(
                    self.freshen_import_type_id_with_context(*type_id, context)?,
                ),
                InstanceExportType::Type(type_id) => InstanceExportType::Type(
                    self.freshen_import_type_id_with_context(*type_id, context)?,
                ),
            };
            exports.insert(name.clone(), export);
        }
        Ok(InstanceType { exports })
    }

    fn instantiate_func(
        &mut self,
        ty: &FuncType,
        context: &mut TransformContext,
    ) -> ParseResult<FuncType> {
        let params = ty
            .params
            .iter()
            .map(|param| self.instantiate_valtype(param, context))
            .collect::<ParseResult<Vec<_>>>()?;
        let result = ty
            .result
            .as_ref()
            .map(|result| self.instantiate_valtype(result, context))
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
        context: &mut TransformContext,
    ) -> ParseResult<FuncType> {
        let params = ty
            .params
            .iter()
            .map(|param| self.freshen_import_valtype(param, context))
            .collect::<ParseResult<Vec<_>>>()?;
        let result = ty
            .result
            .as_ref()
            .map(|result| self.freshen_import_valtype(result, context))
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
        context: &mut TransformContext,
    ) -> ParseResult<ValType> {
        Ok(match ty {
            ValType::Primitive(prim) => ValType::Primitive(prim.clone()),
            ValType::Type(type_id) => ValType::Type(self.instantiate_type_id(*type_id, context)?),
        })
    }

    fn freshen_import_valtype(
        &mut self,
        ty: &ValType,
        context: &mut TransformContext,
    ) -> ParseResult<ValType> {
        Ok(match ty {
            ValType::Primitive(prim) => ValType::Primitive(prim.clone()),
            ValType::Type(type_id) => {
                ValType::Type(self.freshen_import_type_id_with_context(*type_id, context)?)
            }
        })
    }

    fn instantiate_defval(
        &mut self,
        ty: &DefValType,
        context: &mut TransformContext,
    ) -> ParseResult<DefValType> {
        Ok(match ty {
            DefValType::Primitive(prim) => DefValType::Primitive(prim.clone()),
            DefValType::Record(fields) => DefValType::Record(
                fields
                    .iter()
                    .map(|field| {
                        Ok(crate::ir::types::LabelValType::new(
                            field.label.clone(),
                            self.instantiate_valtype(&field.ty, context)?,
                        ))
                    })
                    .collect::<ParseResult<Vec<_>>>()?,
            ),
            DefValType::Variant(cases) => DefValType::Variant(
                cases
                    .iter()
                    .map(|case| {
                        Ok(crate::ir::types::Case::new(
                            case.label.clone(),
                            case.ty
                                .as_ref()
                                .map(|ty| self.instantiate_valtype(ty, context))
                                .transpose()?,
                        ))
                    })
                    .collect::<ParseResult<Vec<_>>>()?,
            ),
            DefValType::Flags(labels) => DefValType::Flags(labels.clone()),
            DefValType::List(ty, len) => {
                DefValType::List(self.instantiate_valtype(ty, context)?, *len)
            }
            DefValType::Own(type_id) => {
                DefValType::Own(self.instantiate_type_id(*type_id, context)?)
            }
            DefValType::Borrow(type_id) => {
                DefValType::Borrow(self.instantiate_type_id(*type_id, context)?)
            }
        })
    }

    fn freshen_import_defval(
        &mut self,
        ty: &DefValType,
        context: &mut TransformContext,
    ) -> ParseResult<DefValType> {
        Ok(match ty {
            DefValType::Primitive(prim) => DefValType::Primitive(prim.clone()),
            DefValType::Record(fields) => DefValType::Record(
                fields
                    .iter()
                    .map(|field| {
                        Ok(crate::ir::types::LabelValType::new(
                            field.label.clone(),
                            self.freshen_import_valtype(&field.ty, context)?,
                        ))
                    })
                    .collect::<ParseResult<Vec<_>>>()?,
            ),
            DefValType::Variant(cases) => DefValType::Variant(
                cases
                    .iter()
                    .map(|case| {
                        Ok(crate::ir::types::Case::new(
                            case.label.clone(),
                            case.ty
                                .as_ref()
                                .map(|ty| self.freshen_import_valtype(ty, context))
                                .transpose()?,
                        ))
                    })
                    .collect::<ParseResult<Vec<_>>>()?,
            ),
            DefValType::Flags(labels) => DefValType::Flags(labels.clone()),
            DefValType::List(ty, len) => {
                DefValType::List(self.freshen_import_valtype(ty, context)?, *len)
            }
            DefValType::Own(type_id) => {
                DefValType::Own(self.freshen_import_type_id_with_context(*type_id, context)?)
            }
            DefValType::Borrow(type_id) => {
                DefValType::Borrow(self.freshen_import_type_id_with_context(*type_id, context)?)
            }
        })
    }

    pub fn validate_effective_type_size(&self, type_id: TypeId) -> ParseResult<()> {
        const EFFECTIVE_TYPE_SIZE_LIMIT: u64 = 1_000_000;

        let mut visiting = VisitTracker::new(self.types.len());
        let size = self.compute_effective_type_size(type_id, &mut visiting)?;
        if size > EFFECTIVE_TYPE_SIZE_LIMIT {
            return Err(ComponentParseError::TypeMismatch(
                "effective type size exceeds the limit".to_owned(),
            ));
        }
        Ok(())
    }

    pub fn validate_current_component_resources(&self, type_id: TypeId) -> ParseResult<()> {
        let mut visiting = VisitTracker::new(self.types.len());
        if self
            .resource_owner_summary(type_id, &mut visiting)?
            .refs_foreign_resource(self.current_scope_id())
        {
            return Err(ComponentParseError::TypeMismatch(
                "refers to resources not defined in the current component".to_owned(),
            ));
        }
        Ok(())
    }

    pub fn validate_component_surface(&self, ty: &ComponentType) -> ParseResult<()> {
        let mut seen = VisitTracker::new(self.types.len());
        self.validate_component_surface_inner(ty, &[], &mut seen)
    }

    pub fn validate_instance_surface(&self, ty: &InstanceType) -> ParseResult<()> {
        let mut seen = VisitTracker::new(self.types.len());
        self.validate_instance_surface_inner(ty, &[], &mut seen)
    }

    pub fn validate_component_type_definition(&self, type_id: TypeId) -> ParseResult<()> {
        if matches!(
            self.types.validation_state(type_id),
            Some(ValidationState::Validated)
        ) {
            return Ok(());
        }
        let Type::Component(component_ty) = self.get_type(type_id)?.clone() else {
            return Err(ComponentParseError::TypeMismatch(
                "Type ID does not refer to any component".to_owned(),
            ));
        };
        let mut seen = VisitTracker::new(self.types.len());
        self.validate_component_surface_inner(&component_ty, &[], &mut seen)?;
        self.types
            .set_validation_state(type_id, ValidationState::Validated);
        Ok(())
    }

    pub fn validate_instance_type_definition(&self, type_id: TypeId) -> ParseResult<()> {
        if matches!(
            self.types.validation_state(type_id),
            Some(ValidationState::Validated)
        ) {
            return Ok(());
        }
        let Type::Instance(instance_ty) = self.get_type(type_id)?.clone() else {
            return Err(ComponentParseError::TypeMismatch(
                "Type ID does not refer to any instance".to_owned(),
            ));
        };
        let mut seen = VisitTracker::new(self.types.len());
        self.validate_instance_surface_inner(&instance_ty, &[], &mut seen)?;
        self.types
            .set_validation_state(type_id, ValidationState::Validated);
        Ok(())
    }

    fn validate_component_surface_inner(
        &self,
        ty: &ComponentType,
        inherited_visible: &[TypeId],
        seen: &mut VisitTracker,
    ) -> ParseResult<()> {
        let import_type_ids = self.collect_component_visible_types(ty, SurfaceRole::Import)?;
        let mut export_type_ids = import_type_ids.clone();
        let mut visiting = VisitTracker::new(self.types.len());
        for export in ty.exports.values() {
            self.extend_export_visible_types(export, &mut export_type_ids, &mut visiting)?;
        }
        let mut import_visible = inherited_visible.to_vec();
        merge_type_ids(&mut import_visible, &import_type_ids);
        let mut export_visible = import_visible.clone();
        merge_type_ids(&mut export_visible, &export_type_ids);

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
        inherited_visible: &[TypeId],
        seen: &mut VisitTracker,
    ) -> ParseResult<()> {
        let mut visible = inherited_visible.to_vec();
        let instance_visible = self.collect_instance_visible_types(ty)?;
        merge_type_ids(&mut visible, &instance_visible);
        for export in ty.exports.values() {
            self.validate_instance_export_surface(export, &visible, seen)?;
        }
        Ok(())
    }

    fn validate_component_import_surface(
        &self,
        import: &ComponentImportType,
        visible: &[TypeId],
        seen: &mut VisitTracker,
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
        visible: &[TypeId],
        seen: &mut VisitTracker,
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
        visible: &[TypeId],
        seen: &mut VisitTracker,
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
        visible: &[TypeId],
        role: SurfaceRole,
        seen: &mut VisitTracker,
    ) -> ParseResult<()> {
        if self.contains_resource_handle(type_id)? {
            return Err(ComponentParseError::TypeMismatch(format!(
                "type not valid to be used as {}",
                role.noun()
            )));
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
        visible: &[TypeId],
        role: SurfaceRole,
        seen: &mut VisitTracker,
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
        visible: &[TypeId],
        role: SurfaceRole,
        seen: &mut VisitTracker,
    ) -> ParseResult<()> {
        if !seen.enter(type_id) {
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
        seen.leave(type_id);
        result
    }

    fn validate_instance_root(
        &self,
        type_id: TypeId,
        visible: &[TypeId],
        role: SurfaceRole,
        seen: &mut VisitTracker,
    ) -> ParseResult<()> {
        if !seen.enter(type_id) {
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
        seen.leave(type_id);
        result
    }

    fn validate_type_definition(
        &self,
        type_id: TypeId,
        visible: &[TypeId],
        seen: &mut VisitTracker,
    ) -> ParseResult<()> {
        if !seen.enter(type_id) {
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
                match self.types.validation_state(type_id).unwrap_or_default() {
                    ValidationState::Validated => Ok(()),
                    ValidationState::InProgress => Ok(()),
                    ValidationState::Unknown => {
                        self.types
                            .set_validation_state(type_id, ValidationState::InProgress);
                        let result =
                            self.validate_component_surface_inner(&component_ty, visible, seen);
                        if result.is_ok() {
                            self.types
                                .set_validation_state(type_id, ValidationState::Validated);
                        } else {
                            self.types
                                .set_validation_state(type_id, ValidationState::Unknown);
                        }
                        result
                    }
                }
            }
            Type::Instance(instance_ty) => {
                match self.types.validation_state(type_id).unwrap_or_default() {
                    ValidationState::Validated => Ok(()),
                    ValidationState::InProgress => Ok(()),
                    ValidationState::Unknown => {
                        self.types
                            .set_validation_state(type_id, ValidationState::InProgress);
                        let result =
                            self.validate_instance_surface_inner(&instance_ty, visible, seen);
                        if result.is_ok() {
                            self.types
                                .set_validation_state(type_id, ValidationState::Validated);
                        } else {
                            self.types
                                .set_validation_state(type_id, ValidationState::Unknown);
                        }
                        result
                    }
                }
            }
        };
        seen.leave(type_id);
        result
    }

    fn validate_type_ref(
        &self,
        type_id: TypeId,
        visible: &[TypeId],
        seen: &mut VisitTracker,
    ) -> ParseResult<()> {
        let ty = self.get_type(type_id)?.clone();
        if self.type_requires_name(&ty) {
            if contains_type_id(visible, type_id)
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
        visible: &[TypeId],
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
        visible: &[TypeId],
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
        visible: &[TypeId],
        seen: &mut VisitTracker,
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
        visible: &[TypeId],
        seen: &mut VisitTracker,
    ) -> ParseResult<()> {
        match val_ty {
            ValType::Primitive(_) => Ok(()),
            ValType::Type(type_id) => self.validate_type_ref(*type_id, visible, seen),
        }
    }

    fn validate_nested_valtype_ref(
        &self,
        val_ty: &ValType,
        visible: &[TypeId],
        seen: &mut VisitTracker,
    ) -> ParseResult<()> {
        match val_ty {
            ValType::Primitive(_) => Ok(()),
            ValType::Type(type_id) => self.validate_nested_type_ref(*type_id, visible, seen),
        }
    }

    fn validate_nested_type_ref(
        &self,
        type_id: TypeId,
        visible: &[TypeId],
        seen: &mut VisitTracker,
    ) -> ParseResult<()> {
        let ty = self.get_type(type_id)?.clone();
        let mut visiting = VisitTracker::new(self.types.len());
        if self.type_requires_nested_name(&ty, &mut visiting)? {
            if contains_type_id(visible, type_id)
                || self.resource_visible_by_identity(type_id, visible)?
                || self.defval_visible_by_structure(type_id, visible)?
            {
                Ok(())
            } else {
                Err(ComponentParseError::TypeMismatch(format!(
                    "surface type requires exported/imported name: {type_id:?} => {ty:?}"
                )))
            }
        } else {
            self.validate_type_definition(type_id, visible, seen)
        }
    }

    fn validate_defval_definition(
        &self,
        def: &DefValType,
        visible: &[TypeId],
        seen: &mut VisitTracker,
    ) -> ParseResult<()> {
        match def {
            DefValType::Primitive(_) => Ok(()),
            DefValType::Record(fields) => {
                for field in fields {
                    self.validate_nested_valtype_ref(&field.ty, visible, seen)?;
                }
                Ok(())
            }
            DefValType::Variant(cases) => {
                for case in cases {
                    if let Some(ty) = &case.ty {
                        self.validate_nested_valtype_ref(ty, visible, seen)?;
                    }
                }
                Ok(())
            }
            DefValType::Flags(_) => Ok(()),
            DefValType::List(ty, _) => self.validate_nested_valtype_ref(ty, visible, seen),
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
            DefValType::Flags(_) => false,
            DefValType::List(_, _) => false,
            DefValType::Own(_) | DefValType::Borrow(_) => false,
        }
    }

    fn type_requires_nested_name(
        &self,
        ty: &Type,
        visiting: &mut VisitTracker,
    ) -> ParseResult<bool> {
        match ty {
            Type::DefVal(def) => self.defval_requires_nested_name(def, visiting),
            Type::Generic(Generic {
                bound: GenericBound::Sub,
                ..
            })
            | Type::Resource(_)
            | Type::Func(_)
            | Type::Component(_)
            | Type::Instance(_) => Ok(true),
            Type::Generic(Generic {
                bound: GenericBound::Eq(_),
                ..
            }) => Ok(false),
        }
    }

    fn defval_requires_nested_name(
        &self,
        def: &DefValType,
        visiting: &mut VisitTracker,
    ) -> ParseResult<bool> {
        match def {
            DefValType::Primitive(_) => Ok(false),
            DefValType::Record(fields) => {
                if !fields
                    .iter()
                    .enumerate()
                    .all(|(index, field)| field.label.0 == index.to_string())
                {
                    return Ok(true);
                }
                for field in fields {
                    if self.nested_valtype_requires_name(&field.ty, visiting)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            DefValType::Variant(cases) => {
                if !self.variant_is_inline(cases) {
                    return Ok(true);
                }
                for case in cases {
                    if let Some(ty) = &case.ty {
                        if self.nested_valtype_requires_name(ty, visiting)? {
                            return Ok(true);
                        }
                    }
                }
                Ok(false)
            }
            DefValType::Flags(_) => Ok(true),
            DefValType::List(ty, _) => self.nested_valtype_requires_name(ty, visiting),
            DefValType::Own(_) | DefValType::Borrow(_) => Ok(false),
        }
    }

    fn nested_valtype_requires_name(
        &self,
        ty: &ValType,
        visiting: &mut VisitTracker,
    ) -> ParseResult<bool> {
        match ty {
            ValType::Primitive(_) => Ok(false),
            ValType::Type(type_id) => self.type_id_requires_nested_name(*type_id, visiting),
        }
    }

    fn type_id_requires_nested_name(
        &self,
        type_id: TypeId,
        visiting: &mut VisitTracker,
    ) -> ParseResult<bool> {
        if !visiting.enter(type_id) {
            return Ok(false);
        }
        let ty = self.get_type(type_id)?.clone();
        let result = self.type_requires_nested_name(&ty, visiting)?;
        visiting.leave(type_id);
        Ok(result)
    }

    fn collect_component_visible_types(
        &self,
        ty: &ComponentType,
        role: SurfaceRole,
    ) -> ParseResult<Vec<TypeId>> {
        let mut visible = Vec::new();
        match role {
            SurfaceRole::Import => {
                let mut visiting = VisitTracker::new(self.types.len());
                for import in ty.imports.values() {
                    if let ComponentImportType::Type { type_id, .. } = import {
                        self.extend_visible_closure_into(*type_id, &mut visible, &mut visiting)?;
                    }
                }
            }
            SurfaceRole::Export => {
                let mut visiting = VisitTracker::new(self.types.len());
                for export in ty.exports.values() {
                    self.extend_export_visible_types(export, &mut visible, &mut visiting)?;
                }
            }
        }
        Ok(visible)
    }

    fn collect_instance_visible_types(&self, ty: &InstanceType) -> ParseResult<Vec<TypeId>> {
        let mut visible = Vec::new();
        let mut visiting = VisitTracker::new(self.types.len());
        for export in ty.exports.values() {
            self.extend_instance_export_visible_types(export, &mut visible, &mut visiting)?;
        }
        Ok(visible)
    }

    fn extend_export_visible_types(
        &self,
        export: &ComponentExportType,
        visible: &mut Vec<TypeId>,
        visiting: &mut VisitTracker,
    ) -> ParseResult<()> {
        match export {
            ComponentExportType::CoreModule(_) => Ok(()),
            ComponentExportType::Component(type_id)
            | ComponentExportType::Instance(type_id)
            | ComponentExportType::Type(type_id)
            | ComponentExportType::Func(type_id) => {
                self.extend_visible_closure_into(*type_id, visible, visiting)
            }
        }
    }

    fn extend_instance_export_visible_types(
        &self,
        export: &InstanceExportType,
        visible: &mut Vec<TypeId>,
        visiting: &mut VisitTracker,
    ) -> ParseResult<()> {
        match export {
            InstanceExportType::CoreModule(_) => Ok(()),
            InstanceExportType::Func(type_id)
            | InstanceExportType::Component(type_id)
            | InstanceExportType::Instance(type_id)
            | InstanceExportType::Type(type_id) => {
                self.extend_visible_closure_into(*type_id, visible, visiting)
            }
        }
    }

    #[cfg(test)]
    fn visible_closure(&self, type_id: TypeId) -> ParseResult<Vec<TypeId>> {
        let mut visible = Vec::new();
        let mut visiting = VisitTracker::new(self.types.len());
        self.extend_visible_closure_into(type_id, &mut visible, &mut visiting)?;
        Ok(visible)
    }

    fn extend_visible_closure_into(
        &self,
        type_id: TypeId,
        visible: &mut Vec<TypeId>,
        visiting: &mut VisitTracker,
    ) -> ParseResult<()> {
        if let Some(closure) = self.types.visible_closure(type_id) {
            merge_type_ids(visible, &closure);
            return Ok(());
        }

        if !visiting.enter(type_id) {
            merge_type_ids(visible, &[type_id]);
            return Ok(());
        }

        if matches!(
            self.get_type(type_id)?,
            Type::DefVal(_)
                | Type::Func(_)
                | Type::Resource(_)
                | Type::Generic(Generic {
                    bound: GenericBound::Sub,
                    ..
                })
        ) {
            visiting.leave(type_id);
            merge_type_ids(visible, &[type_id]);
            return Ok(());
        }

        let mut closure = vec![type_id];
        self.compute_visible_closure(type_id, &mut closure, visiting)?;
        visiting.leave(type_id);
        merge_type_ids(visible, &closure);
        if closure.len() > 1 {
            self.types.set_visible_closure(type_id, closure);
        }
        Ok(())
    }

    fn compute_visible_closure(
        &self,
        type_id: TypeId,
        visible: &mut Vec<TypeId>,
        visiting: &mut VisitTracker,
    ) -> ParseResult<()> {
        match self.get_type(type_id)? {
            Type::Generic(Generic {
                bound: GenericBound::Eq(inner),
                ..
            }) => {
                self.extend_visible_closure_into(*inner, visible, visiting)?;
            }
            Type::Component(component_ty) => {
                for import in component_ty.imports.values() {
                    if let ComponentImportType::Type { type_id, .. } = import {
                        self.extend_visible_closure_into(*type_id, visible, visiting)?;
                    }
                }
                for export in component_ty.exports.values() {
                    self.extend_export_visible_types(export, visible, visiting)?;
                }
            }
            Type::Instance(instance_ty) => {
                for export in instance_ty.exports.values() {
                    self.extend_instance_export_visible_types(export, visible, visiting)?;
                }
            }
            Type::DefVal(_) | Type::Func(_) | Type::Resource(_) | Type::Generic(_) => {}
        }
        Ok(())
    }

    fn contains_resource_handle(&self, type_id: TypeId) -> ParseResult<bool> {
        let mut visiting = VisitTracker::new(self.types.len());
        self.contains_resource_handle_with_tracker(type_id, &mut visiting)
    }

    fn contains_resource_handle_with_tracker(
        &self,
        type_id: TypeId,
        visiting: &mut VisitTracker,
    ) -> ParseResult<bool> {
        if let Some(found) = self.types.contains_resource_handle(type_id) {
            return Ok(found);
        }
        if !visiting.enter(type_id) {
            return Ok(false);
        }

        let result = match self.get_type(type_id)? {
            Type::DefVal(def) => self.defval_contains_resource_handle(def, visiting),
            Type::Generic(Generic {
                bound: GenericBound::Eq(inner),
                ..
            }) => self.contains_resource_handle_with_tracker(*inner, visiting),
            Type::Generic(Generic {
                bound: GenericBound::Sub,
                ..
            })
            | Type::Resource(_)
            | Type::Func(_)
            | Type::Component(_)
            | Type::Instance(_) => Ok(false),
        };
        visiting.leave(type_id);
        let found = result?;
        self.types.set_contains_resource_handle(type_id, found);
        Ok(found)
    }

    fn defval_contains_resource_handle(
        &self,
        def: &DefValType,
        visiting: &mut VisitTracker,
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
            DefValType::Flags(_) => Ok(false),
            DefValType::List(ty, _) => self.valtype_contains_resource_handle(ty, visiting),
            DefValType::Own(_) | DefValType::Borrow(_) => Ok(true),
        }
    }

    fn valtype_contains_resource_handle(
        &self,
        ty: &ValType,
        visiting: &mut VisitTracker,
    ) -> ParseResult<bool> {
        match ty {
            ValType::Primitive(_) => Ok(false),
            ValType::Type(type_id) => {
                self.contains_resource_handle_with_tracker(*type_id, visiting)
            }
        }
    }

    fn resource_owner_summary(
        &self,
        type_id: TypeId,
        visiting: &mut VisitTracker,
    ) -> ParseResult<ResourceOwnerSummary> {
        if let Some(summary) = self.types.resource_owner_summary(type_id) {
            return Ok(summary);
        }

        if !visiting.enter(type_id) {
            return Ok(ResourceOwnerSummary::default());
        }

        let summary = match self.get_type(type_id)? {
            Type::DefVal(def) => self.defval_resource_owner_summary(def, visiting)?,
            Type::Generic(Generic {
                bound: GenericBound::Eq(inner),
                ..
            }) => self.resource_owner_summary(*inner, visiting)?,
            Type::Generic(Generic {
                bound: GenericBound::Sub,
                ..
            }) => ResourceOwnerSummary::default(),
            Type::Resource(resource) => ResourceOwnerSummary::from_owner(resource.owner()),
            Type::Func(func_ty) => {
                let mut summary = ResourceOwnerSummary::default();
                for param in &func_ty.params {
                    summary.merge(&self.valtype_resource_owner_summary(param, visiting)?);
                }
                if let Some(result) = &func_ty.result {
                    summary.merge(&self.valtype_resource_owner_summary(result, visiting)?);
                }
                summary
            }
            Type::Component(component_ty) => {
                let mut summary = ResourceOwnerSummary::default();
                for import in component_ty.imports.values() {
                    if let ComponentImportType::Type { type_id, .. } = import {
                        summary.merge(&self.resource_owner_summary(*type_id, visiting)?);
                    }
                }
                for export in component_ty.exports.values() {
                    match export {
                        ComponentExportType::CoreModule(_) => {}
                        ComponentExportType::Component(type_id)
                        | ComponentExportType::Instance(type_id)
                        | ComponentExportType::Type(type_id)
                        | ComponentExportType::Func(type_id) => {
                            summary.merge(&self.resource_owner_summary(*type_id, visiting)?);
                        }
                    }
                }
                summary
            }
            Type::Instance(instance_ty) => {
                let mut summary = ResourceOwnerSummary::default();
                for export in instance_ty.exports.values() {
                    match export {
                        InstanceExportType::CoreModule(_) => {}
                        InstanceExportType::Component(type_id)
                        | InstanceExportType::Instance(type_id)
                        | InstanceExportType::Type(type_id)
                        | InstanceExportType::Func(type_id) => {
                            summary.merge(&self.resource_owner_summary(*type_id, visiting)?);
                        }
                    }
                }
                summary
            }
        };
        visiting.leave(type_id);
        self.types
            .set_resource_owner_summary(type_id, summary.clone());
        Ok(summary)
    }

    fn defval_resource_owner_summary(
        &self,
        def: &DefValType,
        visiting: &mut VisitTracker,
    ) -> ParseResult<ResourceOwnerSummary> {
        match def {
            DefValType::Primitive(_) => Ok(ResourceOwnerSummary::default()),
            DefValType::Record(fields) => {
                let mut summary = ResourceOwnerSummary::default();
                for field in fields {
                    summary.merge(&self.valtype_resource_owner_summary(&field.ty, visiting)?);
                }
                Ok(summary)
            }
            DefValType::Variant(cases) => {
                let mut summary = ResourceOwnerSummary::default();
                for case in cases {
                    if let Some(ty) = &case.ty {
                        summary.merge(&self.valtype_resource_owner_summary(ty, visiting)?);
                    }
                }
                Ok(summary)
            }
            DefValType::Flags(_) => Ok(ResourceOwnerSummary::default()),
            DefValType::List(ty, _) => self.valtype_resource_owner_summary(ty, visiting),
            DefValType::Own(type_id) | DefValType::Borrow(type_id) => {
                self.resource_owner_summary(*type_id, visiting)
            }
        }
    }

    fn valtype_resource_owner_summary(
        &self,
        ty: &ValType,
        visiting: &mut VisitTracker,
    ) -> ParseResult<ResourceOwnerSummary> {
        match ty {
            ValType::Primitive(_) => Ok(ResourceOwnerSummary::default()),
            ValType::Type(type_id) => self.resource_owner_summary(*type_id, visiting),
        }
    }

    fn variant_is_inline(&self, cases: &[crate::ir::types::Case]) -> bool {
        matches!(
            cases,
            [
                crate::ir::types::Case { label, ty: None },
                crate::ir::types::Case { label: some, ty: Some(_) }
            ] if label.0 == "none" && some.0 == "some"
        ) || matches!(
            cases,
            [
                crate::ir::types::Case { label, .. },
                crate::ir::types::Case { label: err, .. }
            ] if label.0 == "ok" && err.0 == "err"
        )
    }

    fn compute_effective_type_size(
        &self,
        type_id: TypeId,
        visiting: &mut VisitTracker,
    ) -> ParseResult<u64> {
        if let Some(size) = self.types.effective_size(type_id) {
            return Ok(size);
        }
        if !visiting.enter(type_id) {
            return Ok(1);
        }
        let ty = self.get_type(type_id)?.clone();
        let size = self.compute_type_size(&ty, visiting)?;
        visiting.leave(type_id);
        self.types.set_effective_size(type_id, size);
        Ok(size)
    }

    fn compute_type_size(&self, ty: &Type, visiting: &mut VisitTracker) -> ParseResult<u64> {
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
        visiting: &mut VisitTracker,
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
        visiting: &mut VisitTracker,
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
        visiting: &mut VisitTracker,
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
        visiting: &mut VisitTracker,
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
            DefValType::Flags(labels) => Ok((labels.len() as u64).div_ceil(32).max(1)),
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

    fn compute_valtype_size(&self, ty: &ValType, visiting: &mut VisitTracker) -> ParseResult<u64> {
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

#[cfg(test)]
mod tests {
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
