mod child;
mod types;

use crate::component_model::{
    Binding, ComponentExport, ComponentFunction, ComponentIdx, ComponentImport, ComponentType,
    CoreFuncIdx, CoreFunction, CoreGlobalIdx, CoreGlobalRef, CoreInstance, CoreInstanceIdx,
    CoreMemoryIdx, CoreMemoryRef, CoreModule, CoreModuleIdx, CoreTableIdx, CoreTableRef, CoreType,
    CoreTypeIdx, FlattenComponent, FuncIdx, Idx, InlineComponent, Instance, InstanceIdx, Type,
    TypeIdx,
};
#[cfg(feature = "component-gated-feature-value-imports-exports")]
use crate::component_model::{ValueBound, ValueIdx};
use crate::parser::component_model::error::ComponentParseError;
pub use child::ChildValidator;
use std::collections::HashMap;
pub use types::*;

#[derive(Default)]
pub struct LocalStore {
    pub core_modules: Vec<CoreModuleIdx>,
    pub core_instances: Vec<CoreInstanceIdx>,
    pub core_funcs: Vec<CoreFuncIdx>,
    pub components: Vec<ComponentIdx>,
    pub instances: Vec<InstanceIdx>,
    pub core_memories: Vec<CoreMemoryIdx>,
    pub core_tables: Vec<CoreTableIdx>,
    pub core_globals: Vec<CoreGlobalIdx>,
    pub core_types: Vec<CoreTypeIdx>,
    pub functions: Vec<FuncIdx>,
    pub types: Vec<TypeIdx>,
    #[cfg(feature = "component-gated-feature-value-imports-exports")]
    pub values: Vec<ValueIdx>,
    pub imports: HashMap<String, ComponentImport>,
    pub exports: HashMap<String, ComponentExport>,
}

impl LocalStore {
    pub fn make_component_type(&self) -> ComponentType {
        ComponentType {
            imports: Default::default(),
            exports: Default::default(),
            core_types: self.core_types.clone(),
            types: self.types.clone(),
            instances: self.instances.clone(),
        }
    }
}

pub trait Validator: private::Sealed {
    fn get_parent(&self) -> Option<&dyn Validator>;
    fn get_flatten_component(&self) -> &FlattenComponent;
    fn get_flatten_component_mut(&mut self) -> &mut FlattenComponent;
    fn get_local_store(&self) -> &LocalStore;
    fn get_local_store_mut(&mut self) -> &mut LocalStore;

    fn validate_core_module_idx(&self, local: usize) -> Result<CoreModuleIdx, ComponentParseError> {
        self.get_local_store()
            .core_modules
            .get(local)
            .copied()
            .ok_or_else(|| ComponentParseError::InvalidIdx(local, "core module".to_string()))
    }

    fn validate_core_instance_idx(
        &self,
        local: usize,
    ) -> Result<CoreInstanceIdx, ComponentParseError> {
        self.get_local_store()
            .core_instances
            .get(local)
            .copied()
            .ok_or_else(|| ComponentParseError::InvalidIdx(local, "core instance".to_string()))
    }

    fn validate_core_function_idx(&self, local: usize) -> Result<CoreFuncIdx, ComponentParseError> {
        self.get_local_store()
            .core_funcs
            .get(local)
            .copied()
            .ok_or_else(|| ComponentParseError::InvalidIdx(local, "core function".to_string()))
    }
    fn validate_core_memory_idx(&self, local: usize) -> Result<CoreMemoryIdx, ComponentParseError> {
        self.get_local_store()
            .core_memories
            .get(local)
            .copied()
            .ok_or_else(|| ComponentParseError::InvalidIdx(local, "core memory".to_string()))
    }
    fn validate_core_table_idx(&self, local: usize) -> Result<CoreTableIdx, ComponentParseError> {
        self.get_local_store()
            .core_tables
            .get(local)
            .copied()
            .ok_or_else(|| ComponentParseError::InvalidIdx(local, "core table".to_string()))
    }
    fn validate_core_type_idx(&self, local: usize) -> Result<CoreTypeIdx, ComponentParseError> {
        self.get_local_store()
            .core_types
            .get(local)
            .copied()
            .ok_or_else(|| ComponentParseError::InvalidIdx(local, "core type".to_string()))
    }
    fn validate_core_global_idx(&self, local: usize) -> Result<CoreGlobalIdx, ComponentParseError> {
        self.get_local_store()
            .core_globals
            .get(local)
            .copied()
            .ok_or_else(|| ComponentParseError::InvalidIdx(local, "core global".to_string()))
    }
    fn validate_component_idx(&self, local: usize) -> Result<ComponentIdx, ComponentParseError> {
        self.get_local_store()
            .components
            .get(local)
            .copied()
            .ok_or_else(|| ComponentParseError::InvalidIdx(local, "component".to_string()))
    }

    fn validate_function_idx(&self, local: usize) -> Result<FuncIdx, ComponentParseError> {
        self.get_local_store()
            .functions
            .get(local)
            .copied()
            .ok_or_else(|| ComponentParseError::InvalidIdx(local, "function".to_string()))
    }

    fn validate_type_idx(&self, local: usize) -> Result<TypeIdx, ComponentParseError> {
        self.get_local_store()
            .types
            .get(local)
            .copied()
            .ok_or_else(|| ComponentParseError::InvalidIdx(local, "type".to_string()))
    }

    fn validate_instance_idx(&self, local: usize) -> Result<InstanceIdx, ComponentParseError> {
        self.get_local_store()
            .instances
            .get(local)
            .copied()
            .ok_or_else(|| ComponentParseError::InvalidIdx(local, "instance".to_string()))
    }

    #[cfg(feature = "component-gated-feature-value-imports-exports")]
    fn validate_value_idx(&self, local: usize) -> Result<ValueIdx, ComponentParseError> {
        self.get_local_store()
            .values
            .get(local)
            .copied()
            .ok_or_else(|| ComponentParseError::InvalidIdx(local, "value".to_string()))
    }

    fn add_core_module(
        &mut self,
        module: Binding<CoreModule>,
    ) -> Result<CoreModuleIdx, ComponentParseError> {
        let global_idx = self.get_flatten_component().core_modules.len();
        let idx = CoreModuleIdx::new(global_idx);
        self.get_flatten_component_mut().core_modules.push(module);
        self.get_local_store_mut().core_modules.push(idx);
        Ok(idx)
    }

    fn add_core_instance(
        &mut self,
        instance: Binding<CoreInstance>,
    ) -> Result<CoreInstanceIdx, ComponentParseError> {
        let global_idx = self.get_flatten_component().core_instances.len();
        let idx = CoreInstanceIdx::new(global_idx);
        self.get_flatten_component_mut()
            .core_instances
            .push(instance);
        self.get_local_store_mut().core_instances.push(idx);
        Ok(idx)
    }

    fn add_core_func(
        &mut self,
        func: Binding<CoreFunction>,
    ) -> Result<CoreFuncIdx, ComponentParseError> {
        let global_idx = self.get_flatten_component().core_functions.len();
        let idx = CoreFuncIdx::new(global_idx);
        self.get_flatten_component_mut().core_functions.push(func);
        self.get_local_store_mut().core_funcs.push(idx);
        Ok(idx)
    }

    fn add_core_type(&mut self, ty: Binding<CoreType>) -> Result<CoreTypeIdx, ComponentParseError> {
        let global_idx = self.get_flatten_component().core_types.len();
        let idx = CoreTypeIdx::new(global_idx);
        self.get_flatten_component_mut().core_types.push(ty);
        self.get_local_store_mut().core_types.push(idx);
        Ok(idx)
    }

    fn add_core_memory(
        &mut self,
        memory: Binding<CoreMemoryRef>,
    ) -> Result<CoreMemoryIdx, ComponentParseError> {
        let global_idx = self.get_flatten_component().core_memories.len();
        let idx = CoreMemoryIdx::new(global_idx);
        self.get_flatten_component_mut().core_memories.push(memory);
        self.get_local_store_mut().core_memories.push(idx);
        Ok(idx)
    }

    fn add_core_table(
        &mut self,
        table: Binding<CoreTableRef>,
    ) -> Result<CoreTableIdx, ComponentParseError> {
        let global_idx = self.get_flatten_component().core_tables.len();
        let idx = CoreTableIdx::new(global_idx);
        self.get_flatten_component_mut().core_tables.push(table);
        self.get_local_store_mut().core_tables.push(idx);
        Ok(idx)
    }

    fn add_core_global(
        &mut self,
        global: Binding<CoreGlobalRef>,
    ) -> Result<CoreGlobalIdx, ComponentParseError> {
        let global_idx = self.get_flatten_component().core_globals.len();
        let idx = CoreGlobalIdx::new(global_idx);
        self.get_flatten_component_mut().core_globals.push(global);
        self.get_local_store_mut().core_globals.push(idx);
        Ok(idx)
    }

    fn add_component(
        &mut self,
        component: Binding<InlineComponent>,
    ) -> Result<ComponentIdx, ComponentParseError> {
        let global_idx = self.get_flatten_component().components.len();
        let idx = ComponentIdx::new(global_idx);
        self.get_flatten_component_mut().components.push(component);
        self.get_local_store_mut().components.push(idx);
        Ok(idx)
    }

    fn add_instance(
        &mut self,
        instance: Binding<Instance>,
    ) -> Result<InstanceIdx, ComponentParseError> {
        let global_idx = self.get_flatten_component().instances.len();
        let idx = InstanceIdx::new(global_idx);
        self.get_flatten_component_mut().instances.push(instance);
        self.get_local_store_mut().instances.push(idx);
        Ok(idx)
    }

    fn add_func(
        &mut self,
        func: Binding<ComponentFunction>,
    ) -> Result<FuncIdx, ComponentParseError> {
        let global_idx = self.get_flatten_component().functions.len();
        let idx = FuncIdx::new(global_idx);
        self.get_flatten_component_mut().functions.push(func);
        self.get_local_store_mut().functions.push(idx);
        Ok(idx)
    }

    fn add_type(&mut self, ty: Binding<Type>) -> Result<TypeIdx, ComponentParseError> {
        let global_idx = self.get_flatten_component().types.len();
        let idx = TypeIdx::new(global_idx);
        self.get_flatten_component_mut().types.push(ty);
        self.get_local_store_mut().types.push(idx);
        Ok(idx)
    }

    #[cfg(feature = "component-gated-feature-value-imports-exports")]
    fn add_value(&mut self, value: Binding<ValueBound>) -> Result<ValueIdx, ComponentParseError> {
        let global_idx = self.get_flatten_component().values.len();
        let idx = ValueIdx::new(global_idx);
        self.get_flatten_component_mut().values.push(value);
        self.get_local_store_mut().values.push(idx);
        Ok(idx)
    }

    fn add_import(
        &mut self,
        name: String,
        import: ComponentImport,
    ) -> Result<(), ComponentParseError> {
        self.get_local_store_mut().imports.insert(name, import);
        Ok(())
    }

    fn add_export(
        &mut self,
        name: String,
        export: ComponentExport,
    ) -> Result<(), ComponentParseError> {
        self.get_local_store_mut().exports.insert(name, export);
        Ok(())
    }

    fn get_core_module(&self, core_mod_idx: &CoreModuleIdx) -> &CoreModule {
        self.get_flatten_component()
            .get_core_module(core_mod_idx.global())
    }

    fn get_core_type(&self, core_type_idx: &CoreTypeIdx) -> &CoreType {
        self.get_flatten_component()
            .get_core_type(core_type_idx.global())
    }

    fn get_core_instance(&self, core_inst_idx: &CoreInstanceIdx) -> &CoreInstance {
        self.get_flatten_component()
            .get_core_instance(core_inst_idx.global())
    }

    fn get_component(&self, component_idx: &ComponentIdx) -> &InlineComponent {
        self.get_flatten_component()
            .get_component(component_idx.global())
    }

    fn get_instance(&self, instance_idx: &InstanceIdx) -> &Instance {
        self.get_flatten_component()
            .get_instance(instance_idx.global())
    }

    fn get_type(&self, type_idx: &TypeIdx) -> &Type {
        self.get_flatten_component().get_type(type_idx.global())
    }
}

pub struct ComponentValidator<'a> {
    component: &'a mut FlattenComponent,
    store: LocalStore,
}

impl<'a> ComponentValidator<'a> {
    pub fn new(component: &'a mut FlattenComponent) -> Self {
        Self {
            component,
            store: LocalStore::default(),
        }
    }
}

impl Validator for ComponentValidator<'_> {
    #[inline]
    fn get_parent(&self) -> Option<&dyn Validator> {
        None
    }

    #[inline]
    fn get_flatten_component(&self) -> &FlattenComponent {
        self.component
    }

    #[inline]
    fn get_flatten_component_mut(&mut self) -> &mut FlattenComponent {
        self.component
    }

    fn get_local_store(&self) -> &LocalStore {
        &self.store
    }

    fn get_local_store_mut(&mut self) -> &mut LocalStore {
        &mut self.store
    }
}

mod private {
    use crate::parser::component_model::validator::TypeValidator;
    use crate::parser::component_model::{ChildValidator, ComponentValidator};

    pub trait Sealed {}

    // 同じ型に実装
    impl Sealed for ComponentValidator<'_> {}
    impl Sealed for ChildValidator<'_> {}
    impl Sealed for TypeValidator<'_> {}
}
