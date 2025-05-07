use crate::component_model::types::{
    ComponentExportType, ComponentImportType, ComponentType, InstanceType, TyRef, Type, TypeId,
};
use crate::component_model::{
    Component, ComponentExport, ComponentImport, ExportName, ExternDesc, GlobalIdx, ImportName,
    Instance, LocalIdx, ParsedExportName, PlaceholderId, PlaceholderType, PlainName, Relation,
    ResourceId, ScopeId, Sort, StrongUnique,
};
use crate::parser::component_model::{ComponentParseError, ParseResult, Validator};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::marker::PhantomData;
use tracing::trace;
use union_find::UnionFind;

pub struct LocalTypeIndex<T>
where
    T: Clone,
{
    values: Vec<TypeId>,
    pub global_map: HashMap<TypeId, GlobalIdx<T>>,
    pub globals: HashMap<GlobalIdx<T>, Relation<T>>,
}

pub struct ScopeGuard<'a> {
    pub id: ScopeId,
    pub(crate) parent: Option<&'a mut ScopeGuard<'a>>,
    pub components: LocalTypeIndex<Component>,
    pub instances: LocalTypeIndex<Instance>,
    pub types: LocalTypeIndex<Type>,
    import_names: Vec<ImportName>,
    export_names: Vec<ExportName>,
    imports: HashMap<PlaceholderId, ComponentImport>,
    exports: HashMap<PlaceholderId, ComponentExport>,
    import_types: HashMap<PlaceholderId, ComponentImportType>,
    export_types: HashMap<PlaceholderId, ComponentExportType>,
    pub(crate) type_mapping: HashMap<TypeId, TyRef>,
    pub(crate) uf: UnionFind<TypeId>,
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
            types: LocalTypeIndex::<Type>::default(),
            components: LocalTypeIndex::default(),
            import_types: Default::default(),
            export_types: Default::default(),
            imports: Default::default(),
            exports: Default::default(),
            import_names: Default::default(),
            export_names: Default::default(),
            instances: LocalTypeIndex::default(),
            type_mapping: Default::default(),
            uf: UnionFind::new(),
        }
    }

    pub fn make_component(&self) -> Component {
        Component {
            imports: self.imports.clone(),
            exports: self.exports.clone(),
        }
    }

    pub fn make_component_type(&self) -> ComponentType {
        ComponentType {
            imports: self.import_types.clone(),
            exports: self.export_types.clone(),
        }
    }

    pub fn make_instance_type(&self) -> InstanceType {
        todo!()
    }

    pub fn merge_type(&mut self, mapping: HashMap<TypeId, TyRef>) {
        self.type_mapping.extend(mapping);
    }

    pub fn add_type(&mut self, ty: Type) -> TypeId {
        let id = TypeId::new();
        self.type_mapping.insert(id, TyRef::Const(ty));
        id
    }

    /// gets the type id of the given type.
    ///
    /// deferred typeの場合，親タイプを返す．
    pub fn get_type(&mut self, id: TypeId) -> ParseResult<&Type> {
        trace!("get type: {:?}", id);
        let id = self.uf.find(&id);
        self.type_mapping
            .get(&id)
            .ok_or(ComponentParseError::TypeNotFound(id))
            .and_then(|ty| match ty {
                TyRef::Const(ty) => Ok(ty),
                TyRef::Defer(_) => Err(ComponentParseError::TypeNotFound(id)),
            })
    }

    pub fn add_placeholder_type(&mut self, pid: PlaceholderId, inner_id: TypeId) -> TypeId {
        trace!("add placeholder type: {:?}", pid);
        let id = TypeId::new();
        self.type_mapping.insert(id, TyRef::Defer(pid));
        self.uf.union(inner_id, id);
        id
    }

    pub fn add_import(&mut self, pid: PlaceholderId, import: ComponentImport) -> ParseResult<()> {
        trace!("add import: {:?}", pid);
        if self.imports.contains_key(&pid) {
            return Err(ComponentParseError::RedundantImport);
        }
        self.imports.insert(pid, import);
        Ok(())
    }

    pub fn add_import_type(
        &mut self,
        name: ImportName,
        desc: ExternDesc,
    ) -> ParseResult<(PlaceholderId, TypeId)> {
        trace!("add import type: {:?}", name);
        if self.import_names.iter().any(|x| x.weak_eq(&name)) {
            return Err(ComponentParseError::RedundantImport);
        }
        let pid = PlaceholderId::new(self.id, &name, PlaceholderType::Import);

        let (ty, id) = match desc {
            ExternDesc::Component(id) => {
                let new_id = TypeId::new();
                self.uf.union(id, new_id);
                self.components.register(new_id);
                (ComponentImportType::Component(new_id), new_id)
            }
            ExternDesc::Instance(id) => {
                let new_id = TypeId::new();
                self.uf.union(id, new_id);
                self.instances.register(id);
                (ComponentImportType::Instance(new_id), new_id)
            }
            ExternDesc::Eq(id) => {
                let id = self.add_placeholder_type(pid.clone(), id);
                self.types.register(id);
                (ComponentImportType::Type(id), id)
            }
            ExternDesc::Sub => {
                let inner = self.add_type(Type::Resource(ResourceId::new()));
                let id = self.add_placeholder_type(pid.clone(), inner);
                self.types.register(id);
                (ComponentImportType::Sub(id), id)
            }
            ExternDesc::Func(_) => todo!(),
        };

        self.import_types.insert(pid.clone(), ty);
        self.import_names.push(name);
        Ok((pid, id))
    }

    pub fn add_export(&mut self, pid: PlaceholderId, export: ComponentExport) -> ParseResult<()> {
        trace!("add export: {:?}", pid);
        self.exports.insert(pid, export);
        Ok(())
    }

    /// Adds an export type to the scope.
    ///
    /// この関数はsortを取らないので，この関数を呼び出す前にunionしておく必要がある．
    pub fn add_export_type(
        &mut self,
        name: ExportName,
        desc: ExternDesc,
    ) -> ParseResult<(PlaceholderId, ComponentExportType)> {
        trace!("add export type: {:?}", name);
        if self.export_names.iter().any(|x| x.weak_eq(&name)) {
            return Err(ComponentParseError::RedundantImport);
        }
        let pid = PlaceholderId::new(self.id, &name, PlaceholderType::Import);

        let ext = match desc {
            ExternDesc::Component(id) => {
                if name.parsed.is_plain_annotated() {
                    return Err(ComponentParseError::InvalidImport(
                        "annotated export is not allowed".to_string(),
                    ));
                }
                self.components.register(id);
                ComponentExportType::Component(id)
            }
            ExternDesc::Instance(id) => {
                if name.parsed.is_plain_annotated() {
                    return Err(ComponentParseError::InvalidImport(
                        "annotated export is not allowed".to_string(),
                    ));
                }
                self.instances.register(id);
                ComponentExportType::Instance(id)
            }
            ExternDesc::Eq(id) => {
                let super_type = self.get_type(id)?;
                if !super_type.is_function_type() && name.parsed.is_plain_annotated() {
                    return Err(ComponentParseError::InvalidImport(
                        "annotated export is not allowed".to_string(),
                    ));
                }
                let new_id = TypeId::new();
                self.uf.union(id, new_id);
                self.types.register(new_id);
                ComponentExportType::Type(new_id)
            }
            ExternDesc::Sub => {
                if name.parsed.is_plain_annotated() {
                    return Err(ComponentParseError::InvalidImport(
                        "annotated export is not allowed".to_string(),
                    ));
                }
                let id = self.add_type(Type::Resource(ResourceId::new()));
                self.types.register(id);
                ComponentExportType::Sub(id)
            }
            ExternDesc::Func(_id) => {
                if let ParsedExportName::Plain(ref p) = name.parsed {
                    match p {
                        PlainName::Plain(_) => {}
                        PlainName::Constructor(_) => {
                            // Validation of [constructor] names requires that the func returns a (result (own $R)), where $R is the resource labeled r.
                        }
                        PlainName::Method(_, _) => {
                            // Validation of [method] names requires the first parameter of the function to be (param "self" (borrow $R)), where $R is the resource labeled r.
                        }
                        PlainName::Static(_, _) => {}
                    }
                }
                todo!()
            }
        };
        self.export_types.insert(pid, ext.clone());
        self.export_names.push(name);
        Ok((pid, ext))
    }
}

impl<T> Default for LocalTypeIndex<T>
where
    T: Clone,
{
    fn default() -> Self {
        Self {
            values: Vec::new(),
            global_map: HashMap::new(),
            globals: HashMap::new(),
        }
    }
}

impl<T> LocalTypeIndex<T>
where
    T: Clone,
{
    pub fn register(&mut self, id: TypeId) -> LocalIdx<T> {
        let idx = self.values.len();
        trace!("register id: {:?}", id);
        self.values.push(id);
        LocalIdx::new(idx as u32)
    }

    pub fn register_with_data(
        &mut self,
        id: TypeId,
        data: Relation<T>,
    ) -> (LocalIdx<T>, GlobalIdx<T>) {
        let idx = self.values.len();
        self.values.push(id);
        let lid = LocalIdx::new(idx as u32);
        let gid = GlobalIdx::new();
        self.globals.insert(gid, data);
        self.global_map.insert(id, gid);
        (lid, gid)
    }

    pub fn get(&self, idx: LocalIdx<T>) -> ParseResult<TypeId> {
        trace!("get type id: {:?}", idx);
        let Some(id) = self.values.get(idx.get() as usize) else {
            return Err(ComponentParseError::TypeIdxNotFound(idx.get()));
        };
        Ok(*id)
    }

    pub fn get_global_idx(&self, idx: TypeId) -> ParseResult<GlobalIdx<T>> {
        trace!("get global idx: {:?}", idx);
        let Some(gid) = self.global_map.get(&idx) else {
            return Err(ComponentParseError::DataNotFound(idx));
        };
        Ok(*gid)
    }

    pub fn get_data(&self, idx: TypeId) -> ParseResult<&Relation<T>> {
        trace!("get data: {:?}", idx);
        let Some(gid) = self.global_map.get(&idx) else {
            return Err(ComponentParseError::DataNotFound(idx));
        };
        let Some(data) = self.globals.get(gid) else {
            return Err(ComponentParseError::DataNotFound(idx));
        };
        Ok(data)
    }

    pub fn merge(&mut self, other: &Self) {
        self.globals.extend(other.globals.clone())
    }
}
