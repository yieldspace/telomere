use crate::component_model::FlattenComponent;
use crate::parser::component_model::Validator;

pub struct TypeValidator<'a> {
    parent: &'a mut dyn Validator,
}

impl<'a> TypeValidator<'a> {
    pub fn new(parent: &'a mut dyn Validator) -> Self {
        Self { parent }
    }
}

impl Validator for TypeValidator<'_> {
    fn get_parent(&self) -> Option<&dyn Validator> {
        Some(self.parent)
    }

    fn get_flatten_component(&self) -> &FlattenComponent {
        self.parent.get_flatten_component()
    }

    fn get_flatten_component_mut(&mut self) -> &mut FlattenComponent {
        self.parent.get_flatten_component_mut()
    }

    fn get_local_core_module_indexes(&self) -> &Vec<usize> {
        todo!()
    }

    fn get_local_core_instance_indexes(&self) -> &Vec<usize> {
        todo!()
    }

    fn get_local_core_function_indexes(&self) -> &Vec<usize> {
        todo!()
    }

    fn get_local_core_memory_indexes(&self) -> &Vec<usize> {
        todo!()
    }

    fn get_local_core_table_indexes(&self) -> &Vec<usize> {
        todo!()
    }

    fn get_local_core_global_indexes(&self) -> &Vec<usize> {
        todo!()
    }

    fn get_local_core_type_indexes(&self) -> &Vec<usize> {
        todo!()
    }

    fn get_local_component_indexes(&self) -> &Vec<usize> {
        todo!()
    }

    fn get_local_instance_indexes(&self) -> &Vec<usize> {
        todo!()
    }

    fn get_local_function_indexes(&self) -> &Vec<usize> {
        todo!()
    }

    fn get_local_type_indexes(&self) -> &Vec<usize> {
        todo!()
    }

    #[cfg(feature = "component-gated-feature-value-imports-exports")]
    fn get_local_value_indexes(&self) -> &Vec<usize> {
        todo!()
    }

    fn get_local_core_module_indexes_mut(&mut self) -> &mut Vec<usize> {
        todo!()
    }

    fn get_local_core_instance_indexes_mut(&mut self) -> &mut Vec<usize> {
        todo!()
    }

    fn get_local_core_function_indexes_mut(&mut self) -> &mut Vec<usize> {
        todo!()
    }

    fn get_local_core_memory_indexes_mut(&mut self) -> &mut Vec<usize> {
        todo!()
    }

    fn get_local_core_table_indexes_mut(&mut self) -> &mut Vec<usize> {
        todo!()
    }

    fn get_local_core_global_indexes_mut(&mut self) -> &mut Vec<usize> {
        todo!()
    }

    fn get_local_core_type_indexes_mut(&mut self) -> &mut Vec<usize> {
        todo!()
    }

    fn get_local_component_indexes_mut(&mut self) -> &mut Vec<usize> {
        todo!()
    }

    fn get_local_instance_indexes_mut(&mut self) -> &mut Vec<usize> {
        todo!()
    }

    fn get_local_function_indexes_mut(&mut self) -> &mut Vec<usize> {
        todo!()
    }

    fn get_local_type_indexes_mut(&mut self) -> &mut Vec<usize> {
        todo!()
    }

    #[cfg(feature = "component-gated-feature-value-imports-exports")]
    fn get_local_value_indexes_mut(&mut self) -> &mut Vec<usize> {
        todo!()
    }
}
