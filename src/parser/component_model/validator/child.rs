use crate::component_model::{
    Component, ComponentIdx, CoreFuncIdx, CoreInstance, CoreInstanceIdx, CoreMemoryIdx,
    CoreModuleIdx, CoreTypeIdx, FuncIdx, Instance, InstanceIdx, TypeIdx,
};
use crate::parser::component_model::error::ComponentParseError;
use crate::parser::component_model::validator::Validator;
use crate::Module;

pub struct ChildValidator<'a, T>
where
    T: Validator,
{
    parent: &'a mut T,
}

impl<'a, T> ChildValidator<'a, T>
where
    T: Validator,
{
    pub fn new(parent: &'a mut T) -> Self {
        Self { parent }
    }
}

impl<'a, T> Validator for ChildValidator<'a, T>
where
    T: Validator,
{
    fn validate_core_module_idx(&self, local: usize) -> Result<CoreModuleIdx, ComponentParseError> {
        todo!()
    }

    fn validate_core_instance_idx(
        &self,
        local: usize,
    ) -> Result<CoreInstanceIdx, ComponentParseError> {
        todo!()
    }

    fn validate_core_function_idx(&self, local: usize) -> Result<CoreFuncIdx, ComponentParseError> {
        todo!()
    }

    fn validate_component_idx(&self, local: usize) -> Result<ComponentIdx, ComponentParseError> {
        todo!()
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
        todo!()
    }

    fn add_core_instance(
        &mut self,
        instance: CoreInstance,
    ) -> Result<CoreInstanceIdx, ComponentParseError> {
        todo!()
    }

    fn add_component(&mut self, component: Component) -> Result<ComponentIdx, ComponentParseError> {
        todo!()
    }

    fn add_instance(&mut self, instance: Instance) -> Result<InstanceIdx, ComponentParseError> {
        todo!()
    }

    fn get_component(&self, component_idx: &ComponentIdx) -> &Component {
        todo!()
    }
}
