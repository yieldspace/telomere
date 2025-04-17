use crate::component_model::FlattenComponent;
use crate::parser::component_model::validator::Validator;

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
    values: Vec<usize>,
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
            values: vec![],
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

    fn get_flatten_component_mut(&mut self) -> &mut FlattenComponent {
        self.parent.get_flatten_component_mut()
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
