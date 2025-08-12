use crate::Result;
use crate::TypeStore;
use crate::types::TypeId;

pub trait SubType {
    fn is_subtype_of<'a>(&self, store: &'a TypeStore, other: &Self) -> Result<bool>;
}

pub trait EqType {
    fn eq_type<'a>(&self, store: &'a TypeStore, other: &Self) -> Result<bool>;
}

pub struct TypeChecker<'a> {
    store: &'a TypeStore,
}

pub struct TypeCheckState<'a> {
    store: &'a TypeStore,
}

impl<'a> TypeChecker<'a> {
    pub fn new(store: &'a TypeStore) -> Self {
        Self { store }
    }

    /// check if the type is a subtype of another type
    pub fn is_subtype_of<'b>(&self, id: TypeId, other_id: TypeId) -> Result<bool> {
        use TypeId::*;
        match (id, other_id) {
            (Resource(self_id), Resource(other_id)) => Ok(self_id == other_id),
            (Val(self_id), Val(other_id)) => Ok(self_id == other_id),
            (Func(self_id), Func(other_id)) => {
                let self_type = self.store.get_func(&self_id)?;
                let other_type = self.store.get_func(&other_id)?;
                self_type.is_subtype_of(self.store, other_type)
            }
            (Component(self_id), Component(other_id)) => Ok(true),
            (Instance(self_id), Instance(other_id)) => Ok(true),
            (Alias(self_id), Alias(other_id)) => Ok(true),
            _ => return Ok(false),
        }
    }
}
