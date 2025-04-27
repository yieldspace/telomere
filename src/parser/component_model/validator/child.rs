use crate::parser::component_model::Validator;

pub struct ChildValidator<'a, V: Validator> {
    parent: &'a mut V,
}

impl<V> Validator for ChildValidator<'_, V> where V: Validator {
    
}
