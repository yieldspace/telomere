mod store;

use crate::component_model::{
    ComponentExport, ComponentImport, ComponentType, CoreFunc, CoreGlobalRef, CoreInstance,
    CoreInstanceType, CoreMemoryRef, CoreModule, CoreModuleType, CoreTableRef, CoreType,
    ExportName, Func, FuncType, GlobalIdx, ImportName, InlineComponent, Instance, InstanceType,
    Type,
};
use crate::parser::component_model::validator::store::GlobalStore;
use crate::parser::component_model::{ComponentParseError, ParseResult};
use std::collections::HashMap;
pub use store::LocalStore;
use tracing::trace;

pub struct Validator<'a> {
    parent: Option<&'a Validator<'a>>,
    store: LocalStore,
    global_store: GlobalStore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LocalIdx(u32);

impl<'a> Default for Validator<'a> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> Validator<'a> {
    pub fn new() -> Self {
        Self {
            parent: None,
            store: LocalStore::default(),
            global_store: GlobalStore::default(),
        }
    }

    pub fn new_child(parent: &'a Self) -> Self {
        Self {
            parent: Some(parent),
            store: LocalStore::default(),
            global_store: GlobalStore::default(),
        }
    }

    pub fn get_outer(&'a self, ct: u32) -> &'a Validator<'a> {
        if ct == 0 {
            self
        } else {
            self.parent.unwrap().get_outer(ct - 1)
        }
    }

    pub(crate) fn add_import(&mut self, name: ImportName, import: ComponentImport) {
        // todo check name exists
        self.store.imports.insert(name, import);
    }

    pub(crate) fn add_export(&mut self, name: ExportName, export: ComponentExport) {
        // todo check name exists
        self.store.exports.insert(name, export);
    }

    pub(crate) fn get_imports(&self) -> HashMap<ImportName, ComponentImport> {
        self.store.imports.clone()
    }

    pub(crate) fn get_exports(&self) -> HashMap<ExportName, ComponentExport> {
        self.store.exports.clone()
    }

    pub(crate) fn validate_core_module_idx(&self, local: u32) -> ParseResult<LocalIdx> {
        self.store
            .core_modules
            .get(local as usize)
            .ok_or_else(|| {
                ComponentParseError::InvalidType(format!(
                    "CoreModule not found at index {:?}",
                    local
                ))
            })
            .map(|_| LocalIdx(local))
    }

    pub(crate) fn validate_core_instance_idx(&self, local: u32) -> ParseResult<LocalIdx> {
        self.store
            .core_instances
            .get(local as usize)
            .ok_or_else(|| {
                ComponentParseError::InvalidType(format!(
                    "CoreModule not found at index {:?}",
                    local
                ))
            })
            .map(|_| LocalIdx(local))
    }

    pub(crate) fn validate_core_func_idx(&self, local: u32) -> ParseResult<LocalIdx> {
        self.store
            .core_funcs
            .get(local as usize)
            .ok_or_else(|| {
                ComponentParseError::InvalidType(format!("CoreFunc not found at index {:?}", local))
            })
            .map(|_| LocalIdx(local))
    }

    pub(crate) fn validate_core_memory_idx(&self, local: u32) -> ParseResult<LocalIdx> {
        self.store
            .core_memories
            .get(local as usize)
            .ok_or_else(|| {
                ComponentParseError::InvalidType(format!(
                    "CoreMemory not found at index {:?}",
                    local
                ))
            })
            .map(|_| LocalIdx(local))
    }

    pub(crate) fn validate_core_table_idx(&self, local: u32) -> ParseResult<LocalIdx> {
        self.store
            .core_tables
            .get(local as usize)
            .ok_or_else(|| {
                ComponentParseError::InvalidType(format!(
                    "CoreTable not found at index {:?}",
                    local
                ))
            })
            .map(|_| LocalIdx(local))
    }

    pub(crate) fn validate_core_global_idx(&self, local: u32) -> ParseResult<LocalIdx> {
        self.store
            .core_globals
            .get(local as usize)
            .ok_or_else(|| {
                ComponentParseError::InvalidType(format!(
                    "CoreGlobal not found at index {:?}",
                    local
                ))
            })
            .map(|_| LocalIdx(local))
    }

    pub(crate) fn validate_core_type_idx(&self, local: u32) -> ParseResult<LocalIdx> {
        self.store
            .core_types
            .get(local as usize)
            .ok_or_else(|| {
                ComponentParseError::InvalidType(format!("CoreType not found at index {:?}", local))
            })
            .map(|_| LocalIdx(local))
    }

    pub(crate) fn validate_component_idx(&self, local: u32) -> ParseResult<LocalIdx> {
        self.store
            .components
            .get(local as usize)
            .ok_or_else(|| {
                ComponentParseError::InvalidType(format!(
                    "Component not found at index {:?}",
                    local
                ))
            })
            .map(|_| LocalIdx(local))
    }

    pub(crate) fn validate_instance_idx(&self, local: u32) -> ParseResult<LocalIdx> {
        self.store
            .instances
            .get(local as usize)
            .ok_or_else(|| {
                ComponentParseError::InvalidType(format!("Instance not found at index {:?}", local))
            })
            .map(|_| LocalIdx(local))
    }

    pub(crate) fn validate_func_idx(&self, local: u32) -> ParseResult<LocalIdx> {
        self.store
            .functions
            .get(local as usize)
            .ok_or_else(|| {
                ComponentParseError::InvalidType(format!("Function not found at index {:?}", local))
            })
            .map(|_| LocalIdx(local))
    }

    pub(crate) fn validate_type_idx(&self, local: u32) -> ParseResult<LocalIdx> {
        self.store
            .types
            .get(local as usize)
            .ok_or_else(|| {
                ComponentParseError::InvalidType(format!("Type not found at index {:?}", local))
            })
            .map(|_| LocalIdx(local))
    }

    pub(crate) fn get_core_module_type(&self, local_idx: LocalIdx) -> ParseResult<CoreModuleType> {
        self.store
            .core_modules
            .get(local_idx.0 as usize)
            .ok_or_else(|| {
                ComponentParseError::InvalidType(format!(
                    "CoreModuleType not found at index {:?}",
                    local_idx
                ))
            })
            .cloned()
    }
    pub(crate) fn get_core_instance_type(
        &self,
        local_idx: LocalIdx,
    ) -> ParseResult<CoreInstanceType> {
        self.store
            .core_instances
            .get(local_idx.0 as usize)
            .ok_or_else(|| {
                ComponentParseError::InvalidType(format!(
                    "CoreInstanceType not found at index {:?}",
                    local_idx
                ))
            })
            .cloned()
    }
    pub(crate) fn get_core_type(&self, local_idx: LocalIdx) -> ParseResult<CoreType> {
        self.store
            .core_types
            .get(local_idx.0 as usize)
            .ok_or_else(|| {
                ComponentParseError::InvalidType(format!(
                    "CoreType not found at index {:?}",
                    local_idx
                ))
            })
            .cloned()
    }
    pub(crate) fn get_core_memory_type(
        &self,
        local_idx: LocalIdx,
    ) -> ParseResult<crate::common::MemType> {
        self.store
            .core_memories
            .get(local_idx.0 as usize)
            .ok_or_else(|| {
                ComponentParseError::InvalidType(format!(
                    "CoreMemoryType not found at index {:?}",
                    local_idx
                ))
            })
            .cloned()
    }
    pub(crate) fn get_core_table_type(
        &self,
        local_idx: LocalIdx,
    ) -> ParseResult<crate::common::TableType> {
        self.store
            .core_tables
            .get(local_idx.0 as usize)
            .ok_or_else(|| {
                ComponentParseError::InvalidType(format!(
                    "CoreTableType not found at index {:?}",
                    local_idx
                ))
            })
            .cloned()
    }
    pub(crate) fn get_core_global_type(
        &self,
        local_idx: LocalIdx,
    ) -> ParseResult<crate::common::GlobalType> {
        self.store
            .core_globals
            .get(local_idx.0 as usize)
            .ok_or_else(|| {
                ComponentParseError::InvalidType(format!(
                    "CoreGlobalType not found at index {:?}",
                    local_idx
                ))
            })
            .cloned()
    }
    pub(crate) fn get_core_func_type(
        &self,
        local_idx: LocalIdx,
    ) -> ParseResult<crate::common::FuncType> {
        self.store
            .core_funcs
            .get(local_idx.0 as usize)
            .ok_or_else(|| {
                ComponentParseError::InvalidType(format!(
                    "CoreFunctionType not found at index {:?}",
                    local_idx
                ))
            })
            .cloned()
    }
    pub(crate) fn get_component_type(&self, local_idx: LocalIdx) -> ParseResult<ComponentType> {
        self.store
            .components
            .get(local_idx.0 as usize)
            .ok_or_else(|| {
                ComponentParseError::InvalidType(format!(
                    "ComponentType not found at index {:?}",
                    local_idx
                ))
            })
            .cloned()
    }
    pub(crate) fn get_instance_type(&self, local_idx: LocalIdx) -> ParseResult<InstanceType> {
        self.store
            .instances
            .get(local_idx.0 as usize)
            .ok_or_else(|| {
                ComponentParseError::InvalidType(format!(
                    "InstanceType not found at index {:?}",
                    local_idx
                ))
            })
            .cloned()
    }
    pub(crate) fn get_type(&self, local_idx: LocalIdx) -> ParseResult<Type> {
        self.store
            .types
            .get(local_idx.0 as usize)
            .ok_or_else(|| {
                ComponentParseError::InvalidType(format!("Type not found at index {:?}", local_idx))
            })
            .cloned()
    }
    pub(crate) fn get_func_type(&self, local_idx: LocalIdx) -> ParseResult<FuncType> {
        self.store
            .functions
            .get(local_idx.0 as usize)
            .ok_or_else(|| {
                ComponentParseError::InvalidType(format!(
                    "FuncType not found at index {:?}",
                    local_idx
                ))
            })
            .cloned()
    }

    pub(crate) fn add_core_module_type(&mut self, module: CoreModuleType) -> ParseResult<LocalIdx> {
        let idx = self.store.core_modules.len();
        trace!("added core module type {module:?} idx: {idx}");
        self.store.core_modules.push(module);
        Ok(LocalIdx(idx as u32))
    }

    pub(crate) fn add_core_instance_type(
        &mut self,
        instance: CoreInstanceType,
    ) -> ParseResult<LocalIdx> {
        let idx = self.store.core_instances.len();
        trace!("added core instance type {instance:?} idx: {idx}");
        self.store.core_instances.push(instance);
        Ok(LocalIdx(idx as u32))
    }

    pub(crate) fn add_core_memory_type(
        &mut self,
        memory: crate::common::MemType,
    ) -> ParseResult<LocalIdx> {
        let idx = self.store.core_memories.len();
        trace!("added core memory type {memory:?} idx: {idx}");
        self.store.core_memories.push(memory);
        Ok(LocalIdx(idx as u32))
    }

    pub(crate) fn add_core_global_type(
        &mut self,
        global: crate::common::GlobalType,
    ) -> ParseResult<LocalIdx> {
        let idx = self.store.core_globals.len();
        trace!("added core global type {global:?} idx: {idx}");
        self.store.core_globals.push(global);
        Ok(LocalIdx(idx as u32))
    }

    pub(crate) fn add_core_table_type(
        &mut self,
        table: crate::common::TableType,
    ) -> ParseResult<LocalIdx> {
        let idx = self.store.core_tables.len();
        trace!("added core table type {table:?} idx: {idx}");
        self.store.core_tables.push(table);
        Ok(LocalIdx(idx as u32))
    }

    pub(crate) fn add_core_func_type(
        &mut self,
        function: crate::common::FuncType,
    ) -> ParseResult<LocalIdx> {
        let idx = self.store.core_funcs.len();
        trace!("added core func type {function:?} idx: {idx}");
        self.store.core_funcs.push(function);
        Ok(LocalIdx(idx as u32))
    }

    pub(crate) fn add_component_type(&mut self, component: ComponentType) -> ParseResult<LocalIdx> {
        let idx = self.store.components.len();
        trace!("added component type {component:?} idx: {idx}");
        self.store.components.push(component);
        Ok(LocalIdx(idx as u32))
    }

    pub(crate) fn add_instance_type(&mut self, instance: InstanceType) -> ParseResult<LocalIdx> {
        let idx = self.store.instances.len();
        trace!("added instance type {instance:?} idx: {idx}");
        self.store.instances.push(instance);
        Ok(LocalIdx(idx as u32))
    }

    pub(crate) fn add_type(&mut self, ty: Type) -> ParseResult<LocalIdx> {
        let idx = self.store.types.len();
        trace!("added type {ty:?} idx: {idx}");
        self.store.types.push(ty);
        Ok(LocalIdx(idx as u32))
    }

    pub(crate) fn add_func_type(&mut self, func: FuncType) -> ParseResult<LocalIdx> {
        let idx = self.store.functions.len();
        trace!("added func type {func:?} idx: {idx}");
        self.store.functions.push(func);
        Ok(LocalIdx(idx as u32))
    }

    pub(crate) fn register_global_core_module(
        &mut self,
        local_idx: LocalIdx,
        global_idx: GlobalIdx<CoreModule>,
    ) -> ParseResult<()> {
        self.global_store.core_modules.insert(local_idx, global_idx);
        Ok(())
    }

    pub(crate) fn register_global_core_instance(
        &mut self,
        local_idx: LocalIdx,
        global_idx: GlobalIdx<CoreInstance>,
    ) -> ParseResult<()> {
        self.global_store
            .core_instances
            .insert(local_idx, global_idx);
        Ok(())
    }

    pub(crate) fn register_global_core_memory(
        &mut self,
        local_idx: LocalIdx,
        global_idx: GlobalIdx<CoreMemoryRef>,
    ) -> ParseResult<()> {
        self.global_store
            .core_memories
            .insert(local_idx, global_idx);
        Ok(())
    }

    pub(crate) fn register_global_core_table(
        &mut self,
        local_idx: LocalIdx,
        global_idx: GlobalIdx<CoreTableRef>,
    ) -> ParseResult<()> {
        self.global_store.core_tables.insert(local_idx, global_idx);
        Ok(())
    }

    pub(crate) fn register_global_core_global(
        &mut self,
        local_idx: LocalIdx,
        global_idx: GlobalIdx<CoreGlobalRef>,
    ) -> ParseResult<()> {
        self.global_store.core_globals.insert(local_idx, global_idx);
        Ok(())
    }

    pub(crate) fn register_global_core_func(
        &mut self,
        local_idx: LocalIdx,
        global_idx: GlobalIdx<CoreFunc>,
    ) -> ParseResult<()> {
        self.global_store.core_funcs.insert(local_idx, global_idx);
        Ok(())
    }

    pub(crate) fn register_global_component(
        &mut self,
        local_idx: LocalIdx,
        global_idx: GlobalIdx<InlineComponent>,
    ) -> ParseResult<()> {
        self.global_store.components.insert(local_idx, global_idx);
        Ok(())
    }

    pub(crate) fn register_global_instance(
        &mut self,
        local_idx: LocalIdx,
        global_idx: GlobalIdx<Instance>,
    ) -> ParseResult<()> {
        self.global_store.instances.insert(local_idx, global_idx);
        Ok(())
    }

    pub(crate) fn register_global_func(
        &mut self,
        local_idx: LocalIdx,
        global_idx: GlobalIdx<Func>,
    ) -> ParseResult<()> {
        self.global_store.funcs.insert(local_idx, global_idx);
        Ok(())
    }

    pub(crate) fn get_global_core_module(
        &self,
        local_idx: LocalIdx,
    ) -> ParseResult<GlobalIdx<CoreModule>> {
        self.global_store
            .core_modules
            .get(&local_idx)
            .cloned()
            .ok_or_else(|| {
                ComponentParseError::InvalidType(format!(
                    "Global CoreModule not found at index {:?}",
                    local_idx
                ))
            })
    }

    pub(crate) fn get_global_core_instance(
        &self,
        local_idx: LocalIdx,
    ) -> ParseResult<GlobalIdx<CoreInstance>> {
        self.global_store
            .core_instances
            .get(&local_idx)
            .cloned()
            .ok_or_else(|| {
                ComponentParseError::InvalidType(format!(
                    "Global CoreInstance not found at index {:?}",
                    local_idx
                ))
            })
    }

    pub(crate) fn get_global_core_memory(
        &self,
        local_idx: LocalIdx,
    ) -> ParseResult<GlobalIdx<CoreMemoryRef>> {
        self.global_store
            .core_memories
            .get(&local_idx)
            .cloned()
            .ok_or_else(|| {
                ComponentParseError::InvalidType(format!(
                    "Global CoreMemory not found at index {:?}",
                    local_idx
                ))
            })
    }

    pub(crate) fn get_global_core_table(
        &self,
        local_idx: LocalIdx,
    ) -> ParseResult<GlobalIdx<CoreTableRef>> {
        self.global_store
            .core_tables
            .get(&local_idx)
            .cloned()
            .ok_or_else(|| {
                ComponentParseError::InvalidType(format!(
                    "Global CoreTable not found at index {:?}",
                    local_idx
                ))
            })
    }

    pub(crate) fn get_global_core_global(
        &self,
        local_idx: LocalIdx,
    ) -> ParseResult<GlobalIdx<CoreGlobalRef>> {
        self.global_store
            .core_globals
            .get(&local_idx)
            .cloned()
            .ok_or_else(|| {
                ComponentParseError::InvalidType(format!(
                    "Global CoreGlobal not found at index {:?}",
                    local_idx
                ))
            })
    }

    pub(crate) fn get_global_core_func(
        &self,
        local_idx: LocalIdx,
    ) -> ParseResult<GlobalIdx<CoreFunc>> {
        self.global_store
            .core_funcs
            .get(&local_idx)
            .cloned()
            .ok_or_else(|| {
                ComponentParseError::InvalidType(format!(
                    "Global CoreFunc not found at index {:?}",
                    local_idx
                ))
            })
    }

    pub(crate) fn get_global_component(
        &self,
        local_idx: LocalIdx,
    ) -> ParseResult<GlobalIdx<InlineComponent>> {
        self.global_store
            .components
            .get(&local_idx)
            .cloned()
            .ok_or_else(|| {
                ComponentParseError::InvalidType(format!(
                    "Global Component not found at index {:?}",
                    local_idx
                ))
            })
    }

    pub(crate) fn get_global_instance(
        &self,
        local_idx: LocalIdx,
    ) -> ParseResult<GlobalIdx<Instance>> {
        self.global_store
            .instances
            .get(&local_idx)
            .cloned()
            .ok_or_else(|| {
                ComponentParseError::InvalidType(format!(
                    "Global Instance not found at index {:?}",
                    local_idx
                ))
            })
    }

    pub(crate) fn get_global_func(&self, local_idx: LocalIdx) -> ParseResult<GlobalIdx<Func>> {
        self.global_store
            .funcs
            .get(&local_idx)
            .cloned()
            .ok_or_else(|| {
                ComponentParseError::InvalidType(format!(
                    "Global Function not found at index {:?}",
                    local_idx
                ))
            })
    }
}
