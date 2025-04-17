use crate::component_model::FlattenComponent;
use crate::parser::component_model::validator::{LocalStore, Validator};

pub struct ChildValidator<'a> {
    parent: &'a mut dyn Validator,
    store: LocalStore,
}

impl<'a> ChildValidator<'a> {
    pub fn new(parent: &'a mut dyn Validator) -> Self {
        Self {
            parent,
            store: LocalStore::default(),
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

    fn get_local_store(&self) -> &LocalStore {
        &self.store
    }

    fn get_local_store_mut(&mut self) -> &mut LocalStore {
        &mut self.store
    }
}
