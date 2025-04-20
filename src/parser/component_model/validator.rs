mod child;
mod types;

use crate::component_model::{
    Binding, Component, ComponentExport, ComponentFunction, ComponentIdx, ComponentImport,
    CoreFuncIdx, CoreFunction, CoreGlobalIdx, CoreGlobalRef, CoreInstance, CoreInstanceIdx,
    CoreMemoryIdx, CoreMemoryRef, CoreModule, CoreModuleIdx, CoreTableIdx, CoreTableRef, CoreType,
    CoreTypeIdx, CoreTypeRef, ExternDesc, FlattenComponent, FuncIdx, Idx, Instance, InstanceIdx,
    Type, TypeIdx,
};
#[cfg(feature = "component-gated-feature-value-imports-exports")]
use crate::component_model::{ValueBound, ValueIdx};
use crate::parser::component_model::error::ComponentParseError;
use crate::Module;
pub use child::ChildValidator;
pub use types::*;

#[derive(Default)]
pub struct LocalStore {
    pub core_modules: Vec<usize>,
    pub core_instances: Vec<usize>,
    pub core_funcs: Vec<usize>,
    pub components: Vec<usize>,
    pub instances: Vec<usize>,
    pub core_memories: Vec<usize>,
    pub core_tables: Vec<usize>,
    pub core_globals: Vec<usize>,
    pub core_types: Vec<usize>,
    pub functions: Vec<usize>,
    pub types: Vec<usize>,
    #[cfg(feature = "component-gated-feature-value-imports-exports")]
    pub values: Vec<usize>,
    pub imports: Vec<ComponentImport>,
    pub exports: Vec<ComponentExport>,
}

pub trait Validator: private::Sealed {
    fn get_parent(&self) -> Option<&dyn Validator>;
    fn get_flatten_component(&self) -> &FlattenComponent;
    fn get_flatten_component_mut(&mut self) -> &mut FlattenComponent;
    fn get_local_store(&self) -> &LocalStore;
    fn get_local_store_mut(&mut self) -> &mut LocalStore;

    fn validate_core_module_idx(&self, local: usize) -> Result<CoreModuleIdx, ComponentParseError> {
        Ok(CoreModuleIdx::new(
            local,
            *self.get_local_store().core_modules.get(local).unwrap(),
        ))
    }

    fn validate_core_instance_idx(
        &self,
        local: usize,
    ) -> Result<CoreInstanceIdx, ComponentParseError> {
        Ok(CoreInstanceIdx::new(
            local,
            *self.get_local_store().core_instances.get(local).unwrap(),
        ))
    }

    fn validate_core_function_idx(&self, local: usize) -> Result<CoreFuncIdx, ComponentParseError> {
        Ok(CoreFuncIdx::new(
            local,
            *self.get_local_store().core_funcs.get(local).unwrap(),
        ))
    }
    fn validate_core_memory_idx(&self, local: usize) -> Result<CoreMemoryIdx, ComponentParseError> {
        Ok(CoreMemoryIdx::new(
            local,
            *self.get_local_store().core_memories.get(local).unwrap(),
        ))
    }
    fn validate_core_table_idx(&self, local: usize) -> Result<CoreTableIdx, ComponentParseError> {
        Ok(CoreTableIdx::new(
            local,
            *self.get_local_store().core_tables.get(local).unwrap(),
        ))
    }
    fn validate_core_type_idx(&self, local: usize) -> Result<CoreTypeIdx, ComponentParseError> {
        Ok(CoreTypeIdx::new(
            local,
            *self.get_local_store().core_types.get(local).unwrap(),
        ))
    }
    fn validate_core_global_idx(&self, local: usize) -> Result<CoreGlobalIdx, ComponentParseError> {
        Ok(CoreGlobalIdx::new(
            local,
            *self.get_local_store().core_globals.get(local).unwrap(),
        ))
    }
    fn validate_component_idx(&self, local: usize) -> Result<ComponentIdx, ComponentParseError> {
        Ok(ComponentIdx::new(
            local,
            *self.get_local_store().components.get(local).unwrap(),
        ))
    }

    fn validate_function_idx(&self, local: usize) -> Result<FuncIdx, ComponentParseError> {
        Ok(FuncIdx::new(
            local,
            *self.get_local_store().functions.get(local).unwrap(),
        ))
    }

    fn validate_type_idx(&self, local: usize) -> Result<TypeIdx, ComponentParseError> {
        Ok(TypeIdx::new(
            local,
            *self.get_local_store().types.get(local).unwrap(),
        ))
    }

    fn validate_instance_idx(&self, local: usize) -> Result<InstanceIdx, ComponentParseError> {
        Ok(InstanceIdx::new(
            local,
            *self.get_local_store().instances.get(local).unwrap(),
        ))
    }

    #[cfg(feature = "component-gated-feature-value-imports-exports")]
    fn validate_value_idx(&self, local: usize) -> Result<ValueIdx, ComponentParseError> {
        Ok(ValueIdx::new(
            local,
            *self.get_local_store().values.get(local).unwrap(),
        ))
    }

    fn add_core_module(
        &mut self,
        module: Binding<CoreModule>,
    ) -> Result<CoreModuleIdx, ComponentParseError> {
        let global_idx = self.get_flatten_component().core_modules.len();
        let local_idx = self.get_local_store().core_modules.len();
        self.get_flatten_component_mut().core_modules.push(module);
        self.get_local_store_mut().core_modules.push(global_idx);
        let idx = CoreModuleIdx::new(local_idx, global_idx);
        Ok(idx)
    }

    fn add_core_instance(
        &mut self,
        instance: Binding<CoreInstance>,
    ) -> Result<CoreInstanceIdx, ComponentParseError> {
        let global_idx = self.get_flatten_component().core_instances.len();
        let local_idx = self.get_local_store().core_instances.len();
        self.get_flatten_component_mut()
            .core_instances
            .push(instance);
        self.get_local_store_mut().core_instances.push(global_idx);
        let idx = CoreInstanceIdx::new(local_idx, global_idx);
        Ok(idx)
    }

    fn add_core_func(
        &mut self,
        func: Binding<CoreFunction>,
    ) -> Result<CoreFuncIdx, ComponentParseError> {
        let global_idx = self.get_flatten_component().core_functions.len();
        let local_idx = self.get_local_store().core_funcs.len();
        self.get_flatten_component_mut().core_functions.push(func);
        self.get_local_store_mut().core_funcs.push(global_idx);
        let idx = CoreFuncIdx::new(local_idx, global_idx);
        Ok(idx)
    }

    fn add_core_type(&mut self, ty: Binding<CoreType>) -> Result<CoreTypeIdx, ComponentParseError> {
        let global_idx = self.get_flatten_component().core_types.len();
        let local_idx = self.get_local_store().core_types.len();
        self.get_flatten_component_mut().core_types.push(ty);
        self.get_local_store_mut().core_types.push(global_idx);
        let idx = CoreTypeIdx::new(local_idx, global_idx);
        Ok(idx)
    }

    fn add_core_memory(
        &mut self,
        memory: Binding<CoreMemoryRef>,
    ) -> Result<CoreMemoryIdx, ComponentParseError> {
        let global_idx = self.get_flatten_component().core_memories.len();
        let local_idx = self.get_local_store().core_memories.len();
        self.get_flatten_component_mut().core_memories.push(memory);
        self.get_local_store_mut().core_memories.push(global_idx);
        let idx = CoreMemoryIdx::new(local_idx, global_idx);
        Ok(idx)
    }

    fn add_core_table(
        &mut self,
        table: Binding<CoreTableRef>,
    ) -> Result<CoreTableIdx, ComponentParseError> {
        let global_idx = self.get_flatten_component().core_tables.len();
        let local_idx = self.get_local_store().core_tables.len();
        self.get_flatten_component_mut().core_tables.push(table);
        self.get_local_store_mut().core_tables.push(global_idx);
        let idx = CoreTableIdx::new(local_idx, global_idx);
        Ok(idx)
    }

    fn add_core_global(
        &mut self,
        global: Binding<CoreGlobalRef>,
    ) -> Result<CoreGlobalIdx, ComponentParseError> {
        let global_idx = self.get_flatten_component().core_globals.len();
        let local_idx = self.get_local_store().core_globals.len();
        self.get_flatten_component_mut().core_globals.push(global);
        self.get_local_store_mut().core_globals.push(global_idx);
        let idx = CoreGlobalIdx::new(local_idx, global_idx);
        Ok(idx)
    }

    fn add_component(
        &mut self,
        component: Binding<Component>,
    ) -> Result<ComponentIdx, ComponentParseError> {
        let global_idx = self.get_flatten_component().components.len();
        let local_idx = self.get_local_store().components.len();
        self.get_flatten_component_mut().components.push(component);
        self.get_local_store_mut().components.push(global_idx);
        let idx = ComponentIdx::new(local_idx, global_idx);
        Ok(idx)
    }

    fn add_instance(
        &mut self,
        instance: Binding<Instance>,
    ) -> Result<InstanceIdx, ComponentParseError> {
        let global_idx = self.get_flatten_component().instances.len();
        let local_idx = self.get_local_store().instances.len();
        self.get_flatten_component_mut().instances.push(instance);
        self.get_local_store_mut().instances.push(global_idx);
        let idx = InstanceIdx::new(local_idx, global_idx);
        Ok(idx)
    }

    fn add_func(
        &mut self,
        func: Binding<ComponentFunction>,
    ) -> Result<FuncIdx, ComponentParseError> {
        let global_idx = self.get_flatten_component().functions.len();
        let local_idx = self.get_local_store().functions.len();
        self.get_flatten_component_mut().functions.push(func);
        self.get_local_store_mut().functions.push(global_idx);
        let idx = FuncIdx::new(local_idx, global_idx);
        Ok(idx)
    }

    fn add_type(&mut self, ty: Binding<Type>) -> Result<TypeIdx, ComponentParseError> {
        let global_idx = self.get_flatten_component().types.len();
        let local_idx = self.get_local_store().types.len();
        self.get_flatten_component_mut().types.push(ty);
        self.get_local_store_mut().types.push(global_idx);
        let idx = TypeIdx::new(local_idx, global_idx);
        Ok(idx)
    }

    #[cfg(feature = "component-gated-feature-value-imports-exports")]
    fn add_value(&mut self, value: Binding<ValueBound>) -> Result<ValueIdx, ComponentParseError> {
        let global_idx = self.get_flatten_component().values.len();
        let local_idx = self.get_local_store().values.len();
        self.get_flatten_component_mut().values.push(value);
        self.get_local_store_mut().values.push(global_idx);
        let idx = ValueIdx::new(local_idx, global_idx);
        Ok(idx)
    }

    fn add_import(&mut self, import: ComponentImport) -> Result<(), ComponentParseError> {
        self.get_local_store_mut().imports.push(import);
        Ok(())
    }

    fn add_export(&mut self, export: ComponentExport) -> Result<(), ComponentParseError> {
        self.get_local_store_mut().exports.push(export);
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

    fn get_component(&self, component_idx: &ComponentIdx) -> &Component {
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
