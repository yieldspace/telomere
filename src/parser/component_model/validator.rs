mod child;
mod types;

use crate::component_model::{
    Binding, Component, ComponentFunction, ComponentIdx, CoreFuncIdx, CoreFunction, CoreGlobalIdx,
    CoreGlobalRef, CoreInstance, CoreInstanceIdx, CoreMemoryIdx, CoreMemoryRef, CoreModuleIdx,
    CoreTableIdx, CoreTableRef, CoreTypeIdx, CoreTypeRef, FlattenComponent, FuncIdx, Idx, Instance,
    InstanceIdx, Type, TypeIdx,
};
#[cfg(feature = "component-gated-feature-value-imports-exports")]
use crate::component_model::{ValueBound, ValueIdx};
use crate::parser::component_model::error::ComponentParseError;
use crate::Module;
pub use child::ChildValidator;
pub use types::*;

pub(crate) trait Validator {
    fn get_parent(&self) -> Option<&dyn Validator>;
    fn get_flatten_component(&self) -> &FlattenComponent;
    fn get_flatten_component_mut(&mut self) -> &mut FlattenComponent;
    fn get_local_core_module_indexes(&self) -> &Vec<usize>;
    fn get_local_core_instance_indexes(&self) -> &Vec<usize>;
    fn get_local_core_function_indexes(&self) -> &Vec<usize>;
    fn get_local_core_memory_indexes(&self) -> &Vec<usize>;
    fn get_local_core_table_indexes(&self) -> &Vec<usize>;
    fn get_local_core_global_indexes(&self) -> &Vec<usize>;
    fn get_local_core_type_indexes(&self) -> &Vec<usize>;
    fn get_local_component_indexes(&self) -> &Vec<usize>;
    fn get_local_instance_indexes(&self) -> &Vec<usize>;
    fn get_local_function_indexes(&self) -> &Vec<usize>;
    fn get_local_type_indexes(&self) -> &Vec<usize>;
    #[cfg(feature = "component-gated-feature-value-imports-exports")]
    fn get_local_value_indexes(&self) -> &Vec<usize>;
    fn get_local_core_module_indexes_mut(&mut self) -> &mut Vec<usize>;
    fn get_local_core_instance_indexes_mut(&mut self) -> &mut Vec<usize>;
    fn get_local_core_function_indexes_mut(&mut self) -> &mut Vec<usize>;
    fn get_local_core_memory_indexes_mut(&mut self) -> &mut Vec<usize>;
    fn get_local_core_table_indexes_mut(&mut self) -> &mut Vec<usize>;
    fn get_local_core_global_indexes_mut(&mut self) -> &mut Vec<usize>;
    fn get_local_core_type_indexes_mut(&mut self) -> &mut Vec<usize>;
    fn get_local_component_indexes_mut(&mut self) -> &mut Vec<usize>;
    fn get_local_instance_indexes_mut(&mut self) -> &mut Vec<usize>;
    fn get_local_function_indexes_mut(&mut self) -> &mut Vec<usize>;
    fn get_local_type_indexes_mut(&mut self) -> &mut Vec<usize>;
    #[cfg(feature = "component-gated-feature-value-imports-exports")]
    fn get_local_value_indexes_mut(&mut self) -> &mut Vec<usize>;

    fn validate_core_module_idx(&self, local: usize) -> Result<CoreModuleIdx, ComponentParseError> {
        Ok(CoreModuleIdx::new(
            local,
            *self.get_local_core_module_indexes().get(local).unwrap(),
        ))
    }

    fn validate_core_instance_idx(
        &self,
        local: usize,
    ) -> Result<CoreInstanceIdx, ComponentParseError> {
        Ok(CoreInstanceIdx::new(
            local,
            *self.get_local_core_instance_indexes().get(local).unwrap(),
        ))
    }

    fn validate_core_function_idx(&self, local: usize) -> Result<CoreFuncIdx, ComponentParseError> {
        Ok(CoreFuncIdx::new(
            local,
            *self.get_local_core_function_indexes().get(local).unwrap(),
        ))
    }
    fn validate_core_memory_idx(&self, local: usize) -> Result<CoreMemoryIdx, ComponentParseError> {
        Ok(CoreMemoryIdx::new(
            local,
            *self.get_local_core_memory_indexes().get(local).unwrap(),
        ))
    }
    fn validate_core_table_idx(&self, local: usize) -> Result<CoreTableIdx, ComponentParseError> {
        Ok(CoreTableIdx::new(
            local,
            *self.get_local_core_table_indexes().get(local).unwrap(),
        ))
    }
    fn validate_core_type_idx(&self, local: usize) -> Result<CoreTypeIdx, ComponentParseError> {
        Ok(CoreTypeIdx::new(
            local,
            *self.get_local_core_type_indexes().get(local).unwrap(),
        ))
    }
    fn validate_core_global_idx(&self, local: usize) -> Result<CoreGlobalIdx, ComponentParseError> {
        Ok(CoreGlobalIdx::new(
            local,
            *self.get_local_core_global_indexes().get(local).unwrap(),
        ))
    }
    fn validate_component_idx(&self, local: usize) -> Result<ComponentIdx, ComponentParseError> {
        Ok(ComponentIdx::new(
            local,
            *self.get_local_component_indexes().get(local).unwrap(),
        ))
    }

    fn validate_function_idx(&self, local: usize) -> Result<FuncIdx, ComponentParseError> {
        Ok(FuncIdx::new(
            local,
            *self.get_local_function_indexes().get(local).unwrap(),
        ))
    }

    fn validate_type_idx(&self, local: usize) -> Result<TypeIdx, ComponentParseError> {
        Ok(TypeIdx::new(
            local,
            *self.get_local_type_indexes().get(local).unwrap(),
        ))
    }

    fn validate_instance_idx(&self, local: usize) -> Result<InstanceIdx, ComponentParseError> {
        Ok(InstanceIdx::new(
            local,
            *self.get_local_instance_indexes().get(local).unwrap(),
        ))
    }

    #[cfg(feature = "component-gated-feature-value-imports-exports")]
    fn validate_value_idx(&self, local: usize) -> Result<ValueIdx, ComponentParseError> {
        Ok(ValueIdx::new(
            local,
            *self.get_local_value_indexes().get(local).unwrap(),
        ))
    }

    fn add_core_module(
        &mut self,
        module: Binding<Module>,
    ) -> Result<CoreModuleIdx, ComponentParseError> {
        let global_idx = self.get_flatten_component().core_modules.len();
        let local_idx = self.get_local_core_module_indexes().len();
        self.get_flatten_component_mut().core_modules.push(module);
        self.get_local_core_module_indexes_mut().push(global_idx);
        let idx = CoreModuleIdx::new(local_idx, global_idx);
        Ok(idx)
    }

    fn add_core_instance(
        &mut self,
        instance: Binding<CoreInstance>,
    ) -> Result<CoreInstanceIdx, ComponentParseError> {
        let global_idx = self.get_flatten_component().core_instances.len();
        let local_idx = self.get_local_core_instance_indexes().len();
        self.get_flatten_component_mut()
            .core_instances
            .push(instance);
        self.get_local_core_instance_indexes_mut().push(global_idx);
        let idx = CoreInstanceIdx::new(local_idx, global_idx);
        Ok(idx)
    }

    fn add_core_func(
        &mut self,
        func: Binding<CoreFunction>,
    ) -> Result<CoreFuncIdx, ComponentParseError> {
        let global_idx = self.get_flatten_component().core_functions.len();
        let local_idx = self.get_local_core_function_indexes().len();
        self.get_flatten_component_mut().core_functions.push(func);
        self.get_local_core_function_indexes_mut().push(global_idx);
        let idx = CoreFuncIdx::new(local_idx, global_idx);
        Ok(idx)
    }

    fn add_core_type(
        &mut self,
        ty: Binding<CoreTypeRef>,
    ) -> Result<CoreTypeIdx, ComponentParseError> {
        let global_idx = self.get_flatten_component().core_types.len();
        let local_idx = self.get_local_core_type_indexes().len();
        self.get_flatten_component_mut().core_types.push(ty);
        self.get_local_core_type_indexes_mut().push(global_idx);
        let idx = CoreTypeIdx::new(local_idx, global_idx);
        Ok(idx)
    }

    fn add_core_memory(
        &mut self,
        memory: Binding<CoreMemoryRef>,
    ) -> Result<CoreMemoryIdx, ComponentParseError> {
        let global_idx = self.get_flatten_component().core_memories.len();
        let local_idx = self.get_local_core_memory_indexes().len();
        self.get_flatten_component_mut().core_memories.push(memory);
        self.get_local_core_memory_indexes_mut().push(global_idx);
        let idx = CoreMemoryIdx::new(local_idx, global_idx);
        Ok(idx)
    }

    fn add_core_table(
        &mut self,
        table: Binding<CoreTableRef>,
    ) -> Result<CoreTableIdx, ComponentParseError> {
        let global_idx = self.get_flatten_component().core_tables.len();
        let local_idx = self.get_local_core_table_indexes().len();
        self.get_flatten_component_mut().core_tables.push(table);
        self.get_local_core_table_indexes_mut().push(global_idx);
        let idx = CoreTableIdx::new(local_idx, global_idx);
        Ok(idx)
    }

    fn add_core_global(
        &mut self,
        global: Binding<CoreGlobalRef>,
    ) -> Result<CoreGlobalIdx, ComponentParseError> {
        let global_idx = self.get_flatten_component().core_globals.len();
        let local_idx = self.get_local_core_global_indexes().len();
        self.get_flatten_component_mut().core_globals.push(global);
        self.get_local_core_global_indexes_mut().push(global_idx);
        let idx = CoreGlobalIdx::new(local_idx, global_idx);
        Ok(idx)
    }

    fn add_component(
        &mut self,
        component: Binding<Component>,
    ) -> Result<ComponentIdx, ComponentParseError> {
        let global_idx = self.get_flatten_component().components.len();
        let local_idx = self.get_local_component_indexes().len();
        self.get_flatten_component_mut().components.push(component);
        self.get_local_component_indexes_mut().push(global_idx);
        let idx = ComponentIdx::new(local_idx, global_idx);
        Ok(idx)
    }

    fn add_instance(
        &mut self,
        instance: Binding<Instance>,
    ) -> Result<InstanceIdx, ComponentParseError> {
        let global_idx = self.get_flatten_component().instances.len();
        let local_idx = self.get_local_instance_indexes().len();
        self.get_flatten_component_mut().instances.push(instance);
        self.get_local_instance_indexes_mut().push(global_idx);
        let idx = InstanceIdx::new(local_idx, global_idx);
        Ok(idx)
    }

    fn add_func(
        &mut self,
        func: Binding<ComponentFunction>,
    ) -> Result<FuncIdx, ComponentParseError> {
        let global_idx = self.get_flatten_component().functions.len();
        let local_idx = self.get_local_function_indexes().len();
        self.get_flatten_component_mut().functions.push(func);
        self.get_local_function_indexes_mut().push(global_idx);
        let idx = FuncIdx::new(local_idx, global_idx);
        Ok(idx)
    }

    fn add_type(&mut self, ty: Binding<Type>) -> Result<TypeIdx, ComponentParseError> {
        let global_idx = self.get_flatten_component().types.len();
        let local_idx = self.get_local_type_indexes().len();
        self.get_flatten_component_mut().types.push(ty);
        self.get_local_type_indexes_mut().push(global_idx);
        let idx = TypeIdx::new(local_idx, global_idx);
        Ok(idx)
    }

    #[cfg(feature = "component-gated-feature-value-imports-exports")]
    fn add_value(&mut self, value: Binding<ValueBound>) -> Result<ValueIdx, ComponentParseError> {
        let global_idx = self.get_flatten_component().values.len();
        let local_idx = self.get_local_value_indexes().len();
        self.get_flatten_component_mut().values.push(value);
        self.get_local_value_indexes_mut().push(global_idx);
        let idx = ValueIdx::new(local_idx, global_idx);
        Ok(idx)
    }

    fn get_core_module(&self, core_mod_idx: &CoreModuleIdx) -> &Module {
        self.get_flatten_component()
            .get_core_module(core_mod_idx.global())
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
    pub component: &'a mut FlattenComponent,
    core_modules: Vec<usize>,
    core_instances: Vec<usize>,
    core_funcs: Vec<usize>,
    components: Vec<usize>,
    instances: Vec<usize>,
    core_memories: Vec<usize>,
    core_tables: Vec<usize>,
    core_globals: Vec<usize>,
    core_types: Vec<usize>,
    functions: Vec<usize>,
    types: Vec<usize>,
    values: Vec<usize>,
}

impl<'a> ComponentValidator<'a> {
    pub fn new(component: &'a mut FlattenComponent) -> Self {
        Self {
            component,
            core_modules: vec![],
            core_instances: vec![],
            core_funcs: vec![],
            components: vec![],
            instances: vec![],
            core_memories: vec![],
            core_tables: vec![],
            core_globals: vec![],
            core_types: vec![],
            functions: vec![],
            types: vec![],
            values: vec![],
        }
    }
}

impl<'a> Validator for ComponentValidator<'a> {
    fn get_parent(&self) -> Option<&dyn Validator> {
        None
    }

    fn get_flatten_component(&self) -> &FlattenComponent {
        &self.component
    }

    fn get_flatten_component_mut(&mut self) -> &mut FlattenComponent {
        &mut self.component
    }

    fn get_local_core_module_indexes(&self) -> &Vec<usize> {
        &self.core_modules
    }

    fn get_local_core_instance_indexes(&self) -> &Vec<usize> {
        &self.core_instances
    }

    fn get_local_core_function_indexes(&self) -> &Vec<usize> {
        &self.core_funcs
    }

    fn get_local_core_memory_indexes(&self) -> &Vec<usize> {
        &self.core_memories
    }

    fn get_local_core_table_indexes(&self) -> &Vec<usize> {
        &self.core_tables
    }

    fn get_local_core_global_indexes(&self) -> &Vec<usize> {
        &self.core_globals
    }

    fn get_local_core_type_indexes(&self) -> &Vec<usize> {
        &self.core_types
    }

    fn get_local_component_indexes(&self) -> &Vec<usize> {
        &self.components
    }

    fn get_local_instance_indexes(&self) -> &Vec<usize> {
        &self.instances
    }

    fn get_local_function_indexes(&self) -> &Vec<usize> {
        &self.functions
    }

    fn get_local_type_indexes(&self) -> &Vec<usize> {
        &self.types
    }

    #[cfg(feature = "component-gated-feature-value-imports-exports")]
    fn get_local_value_indexes(&self) -> &Vec<usize> {
        &self.values
    }

    fn get_local_core_module_indexes_mut(&mut self) -> &mut Vec<usize> {
        &mut self.core_modules
    }

    fn get_local_core_instance_indexes_mut(&mut self) -> &mut Vec<usize> {
        &mut self.core_instances
    }

    fn get_local_core_function_indexes_mut(&mut self) -> &mut Vec<usize> {
        &mut self.core_funcs
    }

    fn get_local_core_memory_indexes_mut(&mut self) -> &mut Vec<usize> {
        &mut self.core_memories
    }

    fn get_local_core_table_indexes_mut(&mut self) -> &mut Vec<usize> {
        &mut self.core_tables
    }

    fn get_local_core_global_indexes_mut(&mut self) -> &mut Vec<usize> {
        &mut self.core_globals
    }

    fn get_local_core_type_indexes_mut(&mut self) -> &mut Vec<usize> {
        &mut self.core_types
    }

    fn get_local_component_indexes_mut(&mut self) -> &mut Vec<usize> {
        &mut self.components
    }

    fn get_local_instance_indexes_mut(&mut self) -> &mut Vec<usize> {
        &mut self.instances
    }

    fn get_local_function_indexes_mut(&mut self) -> &mut Vec<usize> {
        &mut self.functions
    }

    fn get_local_type_indexes_mut(&mut self) -> &mut Vec<usize> {
        &mut self.types
    }

    #[cfg(feature = "component-gated-feature-value-imports-exports")]
    fn get_local_value_indexes_mut(&mut self) -> &mut Vec<usize> {
        &mut self.values
    }
}
