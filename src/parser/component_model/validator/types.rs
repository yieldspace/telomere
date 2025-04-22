use crate::component_model::{
    Binding, FlattenComponent, Idx, Instance, InstanceIdx, Type, TypeIdx,
};
#[cfg(feature = "component-gated-feature-value-imports-exports")]
use crate::component_model::{ValueBound, ValueIdx};
use crate::parser::component_model::validator::LocalStore;
use crate::parser::component_model::{ComponentParseError, Validator};

pub struct TypeValidator<'a> {
    parent: &'a mut dyn Validator,
    types: Vec<TypeIdx>,
    #[cfg(feature = "component-gated-feature-value-imports-exports")]
    values: Vec<ValueIdx>,
    instances: Vec<InstanceIdx>,
}

/// types, values, instancesのみ新しいindexにするValidator
/// instance type, component typeのみに利用
impl<'a> TypeValidator<'a> {
    pub fn new(parent: &'a mut dyn Validator) -> Self {
        Self {
            parent,
            types: vec![],
            #[cfg(feature = "component-gated-feature-value-imports-exports")]
            values: vec![],
            instances: vec![],
        }
    }
}

impl Validator for TypeValidator<'_> {
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

    fn get_local_store(&self) -> &LocalStore {
        self.parent.get_local_store()
    }

    fn get_local_store_mut(&mut self) -> &mut LocalStore {
        self.parent.get_local_store_mut()
    }

    fn validate_type_idx(&self, local: usize) -> Result<TypeIdx, ComponentParseError> {
        self.types
            .get(local)
            .copied()
            .ok_or_else(|| ComponentParseError::InvalidIdx(local, "type".to_string()))
    }

    fn validate_instance_idx(&self, local: usize) -> Result<InstanceIdx, ComponentParseError> {
        self.instances
            .get(local)
            .copied()
            .ok_or_else(|| ComponentParseError::InvalidIdx(local, "instance".to_string()))
    }

    #[cfg(feature = "component-gated-feature-value-imports-exports")]
    fn validate_value_idx(&self, local: usize) -> Result<ValueIdx, ComponentParseError> {
        self.values
            .get(local)
            .copied()
            .ok_or_else(|| ComponentParseError::InvalidIdx(local, "value".to_string()))
    }

    fn add_instance(
        &mut self,
        instance: Binding<Instance>,
    ) -> Result<InstanceIdx, ComponentParseError> {
        let global_idx = self.get_flatten_component().instances.len();
        let idx = InstanceIdx::new(global_idx);
        self.get_flatten_component_mut().instances.push(instance);
        self.instances.push(idx);
        Ok(idx)
    }

    fn add_type(&mut self, ty: Binding<Type>) -> Result<TypeIdx, ComponentParseError> {
        let global_idx = self.get_flatten_component().types.len();
        let idx = TypeIdx::new(global_idx);
        self.get_flatten_component_mut().types.push(ty);
        self.types.push(idx);
        Ok(idx)
    }

    #[cfg(feature = "component-gated-feature-value-imports-exports")]
    fn add_value(&mut self, value: Binding<ValueBound>) -> Result<ValueIdx, ComponentParseError> {
        let global_idx = self.get_flatten_component().values.len();
        let idx = ValueIdx::new(global_idx);
        self.get_flatten_component_mut().values.push(value);
        self.values.push(idx);
        Ok(idx)
    }
}
