use crate::parser::component_model::{ComponentValidator, DefaultValidator};

pub trait Parent {
    type Validator: DefaultValidator;

    fn get(&self) -> Option<&Self::Validator> {
        None
    }

    fn get_mut(&mut self) -> Option<&mut Self::Validator> {
        None
    }
}

pub struct DefaultParent<'a, V: DefaultValidator> {
    inner: &'a mut V
}

impl<'a, P: DefaultValidator> DefaultParent<'a, P> {
    pub fn new(inner: &'a mut P) -> Self {
        Self { inner }
    }
}

impl<V: DefaultValidator> Parent for DefaultParent<'_, V> {
    type Validator = V;

    fn get(&self) -> Option<&Self::Validator> {
        Some(self.inner)
    }

    fn get_mut(&mut self) -> Option<&mut Self::Validator> {
        Some(self.inner)
    }
}

pub struct EmptyParent;

impl Parent for EmptyParent {
    type Validator = ComponentValidator<'static, Self>;
}
