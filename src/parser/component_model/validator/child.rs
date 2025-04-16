use crate::component_model::{
    Binding, Component, ComponentFunction, ComponentIdx, CoreFuncIdx, CoreFunction, CoreGlobalIdx,
    CoreGlobalRef, CoreInstance, CoreInstanceIdx, CoreMemoryIdx, CoreMemoryRef, CoreModuleIdx,
    CoreTableIdx, CoreTableRef, CoreType, CoreTypeIdx, FlattenComponent, FuncIdx, Instance,
    InstanceIdx, Type, TypeIdx,
};
use crate::parser::component_model::error::ComponentParseError;
use crate::parser::component_model::validator::Validator;
use crate::Module;

pub struct ChildValidator<'a, T>
where
    T: Validator,
{
    parent: &'a mut T,
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
}

impl<'a, T> ChildValidator<'a, T>
where
    T: Validator,
{
    pub fn new(parent: &'a mut T) -> Self {
        Self {
            parent,
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
        }
    }
}

impl<'a, T> Validator for ChildValidator<'a, T>
where
    T: Validator,
{
    fn get_parent(&self) -> Option<&dyn Validator> {
        Some(self.parent)
    }

    fn get_flatten_component(&self) -> &FlattenComponent {
        &self.parent.get_flatten_component()
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

    fn add_core_module(
        &mut self,
        module: Binding<Module>,
    ) -> Result<CoreModuleIdx, ComponentParseError> {
        todo!()
    }

    fn add_core_instance(
        &mut self,
        instance: Binding<CoreInstance>,
    ) -> Result<CoreInstanceIdx, ComponentParseError> {
        todo!()
    }

    fn add_core_func(
        &mut self,
        func: Binding<CoreFunction>,
    ) -> Result<CoreFuncIdx, ComponentParseError> {
        todo!()
    }

    fn add_core_type(&mut self, ty: Binding<CoreType>) -> Result<CoreTypeIdx, ComponentParseError> {
        todo!()
    }

    fn add_core_memory(
        &mut self,
        memory: Binding<CoreMemoryRef>,
    ) -> Result<CoreMemoryIdx, ComponentParseError> {
        todo!()
    }

    fn add_core_table(
        &mut self,
        table: Binding<CoreTableRef>,
    ) -> Result<CoreTableIdx, ComponentParseError> {
        todo!()
    }

    fn add_core_global(
        &mut self,
        global: Binding<CoreGlobalRef>,
    ) -> Result<CoreGlobalIdx, ComponentParseError> {
        todo!()
    }

    fn add_component(
        &mut self,
        component: Binding<Component>,
    ) -> Result<ComponentIdx, ComponentParseError> {
        todo!()
    }

    fn add_instance(
        &mut self,
        instance: Binding<Instance>,
    ) -> Result<InstanceIdx, ComponentParseError> {
        todo!()
    }

    fn add_func(
        &mut self,
        func: Binding<ComponentFunction>,
    ) -> Result<FuncIdx, ComponentParseError> {
        todo!()
    }

    fn add_type(&mut self, ty: Binding<Type>) -> Result<TypeIdx, ComponentParseError> {
        todo!()
    }
}
