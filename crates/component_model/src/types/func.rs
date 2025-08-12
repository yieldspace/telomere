use crate::types::TypeId;

#[derive(Debug)]
pub struct FuncType {
    pub params: Vec<TypeId>,
    pub result: TypeId,
}
