use crate::component_model::{
    Component, ComponentIdx, CoreFuncIdx, CoreInstance, CoreInstanceIdx, CoreModuleIdx,
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
}
