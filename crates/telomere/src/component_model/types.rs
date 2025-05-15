mod component_decl;
mod defval;
mod export_decl;
mod func;
mod import_decl;
mod instance_decl;
mod primitive;
mod sort;
mod val;

use crate::component_model::{ExportId, ImportId, ResourceId, TypeId};
pub use component_decl::*;
pub use defval::*;
pub use export_decl::*;
pub use func::*;
pub use import_decl::*;
pub use instance_decl::*;
pub use primitive::*;
pub use sort::SortType;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
pub use val::*;

#[derive(Clone)]
pub enum Type {
    DefVal(DefValType),
    Generic(Generic),
    Func(FuncType),
    Resource(ResourceId, Option<TypeId>),
    Component(ComponentType),
    Instance(InstanceType),
}

impl Type {
    pub fn is_generic(&self) -> bool {
        matches!(self, Type::Generic(_))
    }
    pub fn is_resource(&self) -> bool {
        matches!(self, Type::Resource(_, _))
    }
    pub fn is_component(&self) -> bool {
        matches!(self, Type::Component(_))
    }
    pub fn is_instance(&self) -> bool {
        matches!(self, Type::Instance(_))
    }
}

#[derive(Clone)]
pub struct Generic {
    pub id: usize,
    pub bound: GenericBound,
}

impl Generic {
    pub fn new(bound: GenericBound) -> Self {
        static GENERIC_ID: AtomicUsize = AtomicUsize::new(0);
        Self {
            id: GENERIC_ID.fetch_add(1, Ordering::Relaxed),
            bound,
        }
    }
}

#[derive(Clone)]
pub enum GenericBound {
    Eq(TypeId),
    Sub(ResourceId),
}

#[derive(Clone)]
pub struct ComponentType {
    pub imports: HashMap<ImportId, Generic>,
    pub exports: HashMap<ExportId, ComponentExportType>,
}

#[derive(Clone)]
pub enum ComponentExportType {
    Component(TypeId),
    Instance(TypeId),
    Type(TypeId),
    NewResource(ResourceId),
}

#[derive(Clone)]
pub struct InstanceType {
    pub exports: HashMap<ExportId, InstanceExportType>,
}

#[derive(Clone)]
pub enum InstanceExportType {
    Component(TypeId),
    Instance(TypeId),
    Resource(ResourceId),
}
