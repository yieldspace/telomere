mod child;

use crate::component_model::{
    Component, ComponentIdx, CoreFuncIdx, CoreInstance, CoreInstanceIdx, CoreMemoryIdx,
    CoreModuleIdx, CoreTypeIdx, FlattenComponent, FuncIdx, Idx, Instance, InstanceIdx, TypeIdx,
};
use crate::parser::component_model::error::ComponentParseError;
use crate::Module;
pub use child::ChildValidator;

pub trait Validator {
    fn validate_core_module_idx(&self, local: usize) -> Result<CoreModuleIdx, ComponentParseError>;

    fn validate_core_instance_idx(
        &self,
        local: usize,
    ) -> Result<CoreInstanceIdx, ComponentParseError>;

    fn validate_core_function_idx(&self, local: usize) -> Result<CoreFuncIdx, ComponentParseError>;

    fn validate_component_idx(&self, local: usize) -> Result<ComponentIdx, ComponentParseError>;

    fn validate_core_memory_idx(&self, local: usize) -> Result<CoreMemoryIdx, ComponentParseError>;

    fn validate_core_type_idx(&self, local: usize) -> Result<CoreTypeIdx, ComponentParseError>;

    fn validate_function_idx(&self, local: usize) -> Result<FuncIdx, ComponentParseError>;

    fn validate_type_idx(&self, local: usize) -> Result<TypeIdx, ComponentParseError>;

    fn validate_instance_idx(&self, local: usize) -> Result<InstanceIdx, ComponentParseError>;

    fn add_core_module(&mut self, module: Module) -> Result<CoreModuleIdx, ComponentParseError>;

    fn add_core_instance(
        &mut self,
        instance: CoreInstance,
    ) -> Result<CoreInstanceIdx, ComponentParseError>;

    fn add_component(&mut self, component: Component) -> Result<ComponentIdx, ComponentParseError>;

    fn add_instance(&mut self, instance: Instance) -> Result<InstanceIdx, ComponentParseError>;

    fn get_component(&self, component_idx: &ComponentIdx) -> &Component;
}

pub struct ComponentValidator<'a> {
    pub component: &'a mut FlattenComponent,
    core_modules: Vec<usize>,
    core_instances: Vec<usize>,
    core_funcs: Vec<usize>,
    components: Vec<usize>,
    instances: Vec<usize>,
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
        }
    }
}

impl<'a> Validator for ComponentValidator<'a> {
    fn validate_core_module_idx(&self, local: usize) -> Result<CoreModuleIdx, ComponentParseError> {
        Ok(CoreModuleIdx::new(
            local,
            *self.core_modules.get(local).unwrap(),
        ))
    }

    fn validate_core_instance_idx(
        &self,
        local: usize,
    ) -> Result<CoreInstanceIdx, ComponentParseError> {
        Ok(CoreInstanceIdx::new(
            local,
            *self.core_instances.get(local).unwrap(),
        ))
    }

    fn validate_core_function_idx(&self, local: usize) -> Result<CoreFuncIdx, ComponentParseError> {
        Ok(CoreFuncIdx::new(
            local,
            *self.core_funcs.get(local).unwrap(),
        ))
    }

    fn validate_component_idx(&self, local: usize) -> Result<ComponentIdx, ComponentParseError> {
        Ok(ComponentIdx::new(
            local,
            *self.components.get(local).unwrap(),
        ))
    }

    fn validate_core_memory_idx(&self, local: usize) -> Result<CoreMemoryIdx, ComponentParseError> {
        todo!()
    }

    fn validate_core_type_idx(&self, local: usize) -> Result<CoreTypeIdx, ComponentParseError> {
        todo!()
    }

    fn validate_function_idx(&self, local: usize) -> Result<FuncIdx, ComponentParseError> {
        todo!()
    }

    fn validate_type_idx(&self, local: usize) -> Result<TypeIdx, ComponentParseError> {
        todo!()
    }

    fn validate_instance_idx(&self, local: usize) -> Result<InstanceIdx, ComponentParseError> {
        todo!()
    }

    fn add_core_module(&mut self, module: Module) -> Result<CoreModuleIdx, ComponentParseError> {
        let global_idx = self.component.core_modules.len();
        let local_idx = self.core_modules.len();
        self.component.core_modules.push(module);
        self.core_modules.push(global_idx);
        let idx = CoreModuleIdx::new(local_idx, global_idx);
        Ok(idx)
    }

    fn add_core_instance(
        &mut self,
        instance: CoreInstance,
    ) -> Result<CoreInstanceIdx, ComponentParseError> {
        let global_idx = self.component.core_instances.len();
        let local_idx = self.core_instances.len();
        self.component.core_instances.push(instance);
        self.core_instances.push(global_idx);
        let idx = CoreInstanceIdx::new(local_idx, global_idx);
        Ok(idx)
    }

    fn add_component(&mut self, component: Component) -> Result<ComponentIdx, ComponentParseError> {
        let global_idx = self.component.components.len();
        let local_idx = self.components.len();
        self.component.components.push(component);
        self.components.push(global_idx);
        let idx = ComponentIdx::new(local_idx, global_idx);
        Ok(idx)
    }

    fn add_instance(&mut self, instance: Instance) -> Result<InstanceIdx, ComponentParseError> {
        let global_idx = self.component.instances.iter().len();
        let local_idx = self.component.instances.len();
        self.component.instances.push(instance);
        self.instances.push(global_idx);
        let idx = InstanceIdx::new(local_idx, global_idx);
        Ok(idx)
    }

    fn get_component(&self, component_idx: &ComponentIdx) -> &Component {
        self.component
            .components
            .get(component_idx.global())
            .unwrap()
    }
}
