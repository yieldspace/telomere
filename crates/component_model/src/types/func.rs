use crate::TypeStore;
use crate::types::check::{SubType, TypeChecker};
use crate::types::{TypeId, ValTypeId};

#[derive(Debug)]
pub struct FuncType {
    pub params: Vec<FuncParamType>,
    pub result: ValTypeId,
}

#[derive(Debug)]
pub struct FuncParamType(String, ValTypeId);

impl SubType for FuncType {
    fn is_subtype_of<'a>(&self, store: &'a TypeStore, other: &Self) -> crate::Result<bool> {
        if self.params.len() != other.params.len() {
            return Ok(false);
        }
        for (left, right) in self.params.iter().zip(other.params.iter()) {
            if !left.is_subtype_of(store, right)? {
                return Ok(false);
            }
        }
        let check = TypeChecker::new(store);
        check.is_subtype_of(TypeId::Val(self.result), TypeId::Val(other.result))
    }
}

impl SubType for FuncParamType {
    fn is_subtype_of<'a>(&self, store: &'a TypeStore, other: &Self) -> crate::Result<bool> {
        let check = TypeChecker::new(store);
        check.is_subtype_of(TypeId::Val(self.1), TypeId::Val(other.1))
    }
}
