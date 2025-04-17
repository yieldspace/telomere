use crate::component_model::FlattenComponent;
use crate::parser::component_model::validator::Validator;

pub struct ChildValidator<'a> {
    parent: &'a mut dyn Validator,
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

impl<'a> ChildValidator<'a> {
    pub fn new(parent: &'a mut dyn Validator) -> Self {
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

impl Validator for ChildValidator<'_> {
    #[inline]
    fn get_parent(&self) -> Option<&dyn Validator> {
        Some(self.parent)
    }

    #[inline]
    fn get_flatten_component(&self) -> &FlattenComponent {
        self.parent.get_flatten_component()
    }

    #[inline]
    fn get_flatten_component_mut(&mut self) -> &mut FlattenComponent {
        self.parent.get_flatten_component_mut()
    }

    #[inline]
    fn get_local_core_module_indexes(&self) -> &Vec<usize> {
        &self.core_modules
    }

    #[inline]
    fn get_local_core_instance_indexes(&self) -> &Vec<usize> {
        &self.core_instances
    }

    #[inline]
    fn get_local_core_function_indexes(&self) -> &Vec<usize> {
        &self.core_funcs
    }

    #[inline]
    fn get_local_core_memory_indexes(&self) -> &Vec<usize> {
        &self.core_memories
    }

    #[inline]
    fn get_local_core_table_indexes(&self) -> &Vec<usize> {
        &self.core_tables
    }

    #[inline]
    fn get_local_core_global_indexes(&self) -> &Vec<usize> {
        &self.core_globals
    }

    #[inline]
    fn get_local_core_type_indexes(&self) -> &Vec<usize> {
        &self.core_types
    }

    #[inline]
    fn get_local_component_indexes(&self) -> &Vec<usize> {
        &self.components
    }

    #[inline]
    fn get_local_instance_indexes(&self) -> &Vec<usize> {
        &self.instances
    }

    #[inline]
    fn get_local_function_indexes(&self) -> &Vec<usize> {
        &self.functions
    }

    #[inline]
    fn get_local_type_indexes(&self) -> &Vec<usize> {
        &self.types
    }

    #[inline]
    #[cfg(feature = "component-gated-feature-value-imports-exports")]
    fn get_local_value_indexes(&self) -> &Vec<usize> {
        &self.values
    }

    #[inline]
    fn get_local_core_module_indexes_mut(&mut self) -> &mut Vec<usize> {
        &mut self.core_modules
    }

    #[inline]
    fn get_local_core_instance_indexes_mut(&mut self) -> &mut Vec<usize> {
        &mut self.core_instances
    }

    #[inline]
    fn get_local_core_function_indexes_mut(&mut self) -> &mut Vec<usize> {
        &mut self.core_funcs
    }

    #[inline]
    fn get_local_core_memory_indexes_mut(&mut self) -> &mut Vec<usize> {
        &mut self.core_memories
    }

    #[inline]
    fn get_local_core_table_indexes_mut(&mut self) -> &mut Vec<usize> {
        &mut self.core_tables
    }

    #[inline]
    fn get_local_core_global_indexes_mut(&mut self) -> &mut Vec<usize> {
        &mut self.core_globals
    }

    #[inline]
    fn get_local_core_type_indexes_mut(&mut self) -> &mut Vec<usize> {
        &mut self.core_types
    }

    #[inline]
    fn get_local_component_indexes_mut(&mut self) -> &mut Vec<usize> {
        &mut self.components
    }

    #[inline]
    fn get_local_instance_indexes_mut(&mut self) -> &mut Vec<usize> {
        &mut self.instances
    }

    #[inline]
    fn get_local_function_indexes_mut(&mut self) -> &mut Vec<usize> {
        &mut self.functions
    }

    #[inline]
    fn get_local_type_indexes_mut(&mut self) -> &mut Vec<usize> {
        &mut self.types
    }

    #[inline]
    #[cfg(feature = "component-gated-feature-value-imports-exports")]
    fn get_local_value_indexes_mut(&mut self) -> &mut Vec<usize> {
        &mut self.values
    }
}
