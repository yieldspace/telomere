use crate::component_model::types::{ExportDecl, InstanceDecl, Type, TypeKind};
use crate::component_model::{Alias, CoreType};
use crate::parser::component::sort::TypeTable;

pub struct InstanceTypeSort<'a, T: TypeTable> {
    parent: Option<&'a mut T>,
    types: Vec<Type>,
}

impl<'a, T> InstanceTypeSort<'a, T>
where
    T: TypeTable,
{
    pub fn new() -> Self {
        Self {
            parent: None,
            types: vec![],
        }
    }

    pub fn with_parent(parent: &'a mut T) -> Self {
        Self {
            parent: Some(parent),
            types: vec![],
        }
    }

    pub fn add_type(&mut self, ty: Type) {
        self.types.push(ty);
    }

    pub fn add_instance_decl(&mut self, decl: InstanceDecl) {}
}

impl<'a, T> TypeTable for InstanceTypeSort<'a, T>
where
    T: TypeTable,
{
    fn get_type(&self, idx: usize) -> Option<TypeKind> {
        todo!()
    }
}
