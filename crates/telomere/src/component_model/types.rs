use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use crate::component_model::{ExportId, ImportId, ResourceId, TypeId};

#[derive(Clone)]
pub enum Type {
    Generic(Generic),
    Resource(ResourceId),
    Component(ComponentType),
    Instance(InstanceType),
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

#[derive(Debug, Clone)]
pub enum SortType {
    // Core(),
    Component,
    Func,
    Type,
    Instance,
}
