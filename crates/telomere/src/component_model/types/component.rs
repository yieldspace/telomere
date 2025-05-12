use crate::component_model::types::{InstanceExportType, InstanceType, TyRef, TypeId};
use crate::component_model::PlaceholderId;
use std::collections::HashMap;
use tracing::trace;
use crate::component_model::types::placeholder::{ResolveContext, TypeKind};
use crate::parser::component_model::ParseResult;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentType {
    pub imports: HashMap<PlaceholderId, ComponentImportType>,
    pub exports: HashMap<PlaceholderId, ComponentExportType>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComponentExportType {
    Component(TypeId),
    Instance(TypeId),
    Type(TypeId),
    Sub(TypeId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComponentImportType {
    Component(TypeId),
    Instance(TypeId),
    Type(TypeId),
    Sub(TypeId),
}

impl From<ComponentType> for InstanceType {
    fn from(value: ComponentType) -> Self {
        Self::new(value.exports.into_iter().map(|(k, v)| (k, v.into())).collect())
    }
}

impl From<ComponentExportType> for InstanceExportType {
    fn from(value: ComponentExportType) -> Self {
        match value {
            ComponentExportType::Component(id) => InstanceExportType::Component(id),
            ComponentExportType::Instance(id) => InstanceExportType::Instance(id),
            ComponentExportType::Type(id) => InstanceExportType::Type(id),
            ComponentExportType::Sub(id) => InstanceExportType::Sub(id),
        }
    }
}

impl ComponentImportType {
    pub fn get_type_id(&self) -> TypeId {
        match self {
            ComponentImportType::Component(id) => *id,
            ComponentImportType::Instance(id) => *id,
            ComponentImportType::Type(id) => *id,
            ComponentImportType::Sub(id) => *id,
        }
    }
}

impl TypeKind for ComponentType {
    fn resolve(&mut self, ctx: &mut ResolveContext) -> ParseResult<()> {
        trace!("resolving component type");
        for (_, ty) in self.exports.iter_mut() {
            match ty {
                ComponentExportType::Component(id) => {
                    if let TyRef::Defer(pid, _) = ctx.scope.get_tyref(*id)?.clone() {
                        if let Some(new_id) = ctx.get_new_type(&pid) {
                            ctx.scope.assert_type_eq_or_super(*id, new_id)?;
                            *ty = ComponentExportType::Component(new_id);
                        }
                    }
                }
                ComponentExportType::Instance(id) => {
                    if let TyRef::Defer(pid, _) = ctx.scope.get_tyref(*id)?.clone() {
                        if let Some(new_id) = ctx.get_new_type(&pid) {
                            ctx.scope.assert_type_eq_or_super(*id, new_id)?;
                            *ty = ComponentExportType::Instance(new_id);
                        }
                    }
                }
                ComponentExportType::Type(id) => {
                    if let TyRef::Defer(pid, _) = ctx.scope.get_tyref(*id)?.clone() {
                        if let Some(new_id) = ctx.get_new_type(&pid) {
                            ctx.scope.assert_type_eq_or_super(*id, new_id)?;
                            *ty = ComponentExportType::Type(new_id);
                        }
                    }
                }
                ComponentExportType::Sub(id) => {
                    if let TyRef::Defer(pid, _) = ctx.scope.get_tyref(*id)?.clone() {
                        if let Some(new_id) = ctx.get_new_type(&pid) {
                            ctx.scope.assert_type_eq_or_super(*id, new_id)?;
                            *ty = ComponentExportType::Sub(new_id);
                        }
                    }
                }
            }
        }
        for (_, ty) in self.imports.iter_mut() {
            match ty {
                ComponentImportType::Component(id) => {
                    if let TyRef::Defer(pid, _) = ctx.scope.get_tyref(*id)?.clone() {
                        if let Some(new_id) = ctx.get_new_type(&pid) {
                            ctx.scope.assert_type_eq_or_super(*id, new_id)?;
                            *ty = ComponentImportType::Component(new_id);
                        }
                    }
                }
                ComponentImportType::Instance(id) => {
                    if let TyRef::Defer(pid, _) = ctx.scope.get_tyref(*id)?.clone() {
                        if let Some(new_id) = ctx.get_new_type(&pid) {
                            ctx.scope.assert_type_eq_or_super(*id, new_id)?;
                            *ty = ComponentImportType::Instance(new_id);
                        }
                    }
                }
                ComponentImportType::Type(id) => {
                    if let TyRef::Defer(pid, _) = ctx.scope.get_tyref(*id)?.clone() {
                        if let Some(new_id) = ctx.get_new_type(&pid) {
                            ctx.scope.assert_type_eq_or_super(*id, new_id)?;
                            *ty = ComponentImportType::Type(new_id);
                        }
                    }
                }
                ComponentImportType::Sub(id) => {
                    if let TyRef::Defer(pid, _) = ctx.scope.get_tyref(*id)?.clone() {
                        if let Some(new_id) = ctx.get_new_type(&pid) {
                            ctx.scope.assert_type_eq_or_super(*id, new_id)?;
                            *ty = ComponentImportType::Sub(new_id);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn is_eq_or_super_type_of(&self, other: &Self) -> bool {
        todo!()
    }
}