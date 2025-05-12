use crate::component_model::types::{TyRef, TypeId};
use crate::component_model::{ExportName, PlaceholderId};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use tracing::trace;
use crate::component_model::types::placeholder::{ResolveContext, TypeKind};
use crate::parser::component_model::ParseResult;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstanceType {
    exports: HashMap<PlaceholderId, InstanceExportType>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstanceExportType {
    Component(TypeId),
    Instance(TypeId),
    Type(TypeId),
    Sub(TypeId),
}

impl InstanceType {
    pub fn new(exports: HashMap<PlaceholderId, InstanceExportType>) -> Self {
        Self { exports }
    }

    pub fn get_export(&self, name: &ExportName) -> Option<(&PlaceholderId, &InstanceExportType)> {
        let hash = {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            name.hash(&mut hasher);
            hasher.finish()
        };
        self.exports
            .iter()
            .find(|(pid, _)| pid.name_hash() == hash)
    }
}

impl TypeKind for InstanceType {
    fn resolve(&mut self, ctx: &mut ResolveContext) -> ParseResult<()> {
        trace!("resolving instance type");
        for (_, ty) in self.exports.iter_mut() {
            match ty {
                InstanceExportType::Component(id) => {
                    if let TyRef::Defer(pid, _) = ctx.scope.get_tyref(*id)?.clone() {
                        if let Some(new_id) = ctx.get_new_type(&pid) {
                            ctx.scope.assert_type_eq_or_super(*id, new_id)?;
                            *ty = InstanceExportType::Component(new_id);
                        }
                    }
                }
                InstanceExportType::Instance(id) => {
                    if let TyRef::Defer(pid, _) = ctx.scope.get_tyref(*id)?.clone() {
                        if let Some(new_id) = ctx.get_new_type(&pid) {
                            ctx.scope.assert_type_eq_or_super(*id, new_id)?;
                            *ty = InstanceExportType::Instance(new_id);
                        }
                    }
                }
                InstanceExportType::Type(id) => {
                    if let TyRef::Defer(pid, _) = ctx.scope.get_tyref(*id)?.clone() {
                        if let Some(new_id) = ctx.get_new_type(&pid) {
                            ctx.scope.assert_type_eq_or_super(*id, new_id)?;
                            *ty = InstanceExportType::Type(new_id);
                        }
                    }
                }
                InstanceExportType::Sub(id) => {
                    if let TyRef::Defer(pid, _) = ctx.scope.get_tyref(*id)?.clone() {
                        if let Some(new_id) = ctx.get_new_type(&pid) {
                            ctx.scope.assert_type_eq_or_super(*id, new_id)?;
                            *ty = InstanceExportType::Sub(new_id);
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