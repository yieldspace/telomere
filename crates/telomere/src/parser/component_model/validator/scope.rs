use crate::component_model::types::{ComponentExportType, Generic, GenericBound, InstanceType, SortType, TyRef, Type, TypeId};
use crate::component_model::{Component, ComponentExport, ComponentImport, ExportId, ExportName, ExternDesc, GlobalIdx, ImportId, ImportName, Instance, LocalIdx, ParsedExportName, PlaceholderId, PlaceholderType, PlainName, Relation, ResourceId, ScopeId, Sort, StrongUnique};
use crate::parser::component_model::{ComponentParseError, ParseResult, Validator};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fmt::{Debug, Formatter};
use std::marker::PhantomData;
use tracing::trace;
use union_find::UnionFind;

#[derive(Default)]
struct Locals {
    types: Vec<TypeId>,
    components: Vec<TypeId>,
    instances: Vec<TypeId>,
    funcs: Vec<TypeId>,
}

pub struct ScopeGuard<'a> {
    pub id: ScopeId,
    pub(crate) parent: Option<&'a mut ScopeGuard<'a>>,
    import_types: HashMap<ImportId, Generic>,
    export_types: HashMap<ExportId, ComponentExportType>,
    export_type_map: HashMap<TypeId, ExportId>,
    types: HashMap<TypeId, Type>,
    locals: Locals,
}

impl<'a> ScopeGuard<'a> {
    pub fn new(parent: Option<&'a mut ScopeGuard<'a>>) -> Self {
        let depth = if let Some(ref parent) = parent {
            parent.id.depth() + 1
        } else {
            0
        };
        Self {
            id: ScopeId::new(depth),
            parent,
            import_types: Default::default(),
            export_types: Default::default(),
            export_type_map: Default::default(),
            types: Default::default(),
            locals: Locals::default(),
        }
    }
    
    pub fn new_type(&mut self, ty: Type) -> TypeId {
        let id = TypeId::new();
        self.types.insert(id, ty);
        id
    }
    
    pub fn add_import(&mut self, name: &ImportName, bound: GenericBound) {
        let generic = Generic::new(bound);
        let _id = self.new_type(Type::Generic(generic.clone()));
        self.import_types.insert(ImportId::new(name), generic);
    }

    pub fn add_export(&mut self, name: &ExportName, sort_type: SortType, id: TypeId, desc: Option<ExternDesc>) {
        if let Some(x) = self.export_type_map.get(&id) {
            if let Some(ComponentExportType::NewResource(id)) = self.export_types.get(x).unwrap() {
                
            }
        }
        match sort_type {
            SortType::Component => {
                self.locals.components.push(id);
            }
            SortType::Func => {
                self.locals.funcs.push(id);
            }
            SortType::Type => {
                self.locals.types.push(id);
            }
            SortType::Instance => {
                self.locals.instances.push(id);
            }
        }
        match (sort_type, desc) {
            (SortType::Component, Some(ExternDesc::Component(overwrite_id))) => {
                self.export_types.insert(ExportId::new(name), ComponentExportType::Component(overwrite_id));
            }
            (SortType::Instance, Some(ExternDesc::Instance(overwrite_id))) => {
                self.export_types.insert(ExportId::new(name), ComponentExportType::Instance(overwrite_id));
            }
            (SortType::Type, Some(ExternDesc::Eq(overwrite_id))) => {
                self.export_types.insert(ExportId::new(name), ComponentExportType::Type(overwrite_id));
            }
            (SortType::Type, Some(ExternDesc::Sub)) => {
                let rid = ResourceId::new();
                let id = self.new_type(Type::Resource(rid));
                self.export_types.insert(ExportId::new(name), ComponentExportType::NewResource(rid));
                self.locals.types.push(id);
            }
            (SortType::Func, Some(ExternDesc::Func(_))) => {
                
            }
            _ => unreachable!()
        }
    }
    
    pub fn add_export_in_type(&mut self, name: &ExportName, desc: ExternDesc) {
        match desc {
            ExternDesc::Component(id) => {
                self.export_types.insert(ExportId::new(name), ComponentExportType::Component(id));
                self.locals.components.push(id);
            }
            ExternDesc::Instance(id) => {
                self.export_types.insert(ExportId::new(name), ComponentExportType::Instance(id));
                self.locals.instances.push(id);
            }
            ExternDesc::Eq(id) => {
                self.export_types.insert(ExportId::new(name), ComponentExportType::Type(id));
            }
            ExternDesc::Sub => {
                let rid = ResourceId::new();
                let id = self.new_type(Type::Resource(rid));
                self.export_types.insert(ExportId::new(name), ComponentExportType::NewResource(rid));
                self.locals.types.push(id);
            }
            ExternDesc::Func(_) => {}
        }
    }
}
