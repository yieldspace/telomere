use crate::runtime::component_model::{Component, ComponentInstantiated, CoreFunction};

pub trait Idx<T> {
    fn get<'a>(&self, component: &'a Component) -> &'a T;
}

#[derive(Debug)]
pub struct TypeIdx(usize);

#[derive(Debug)]
pub struct CoreFuncIdx(usize);

impl Idx<CoreFunction> for CoreFuncIdx {
    fn get<'a>(&self, component: &'a Component) -> &'a CoreFunction {
        component.get_core_function(self.0)
    }
}

#[derive(Debug)]
pub struct CoreMemoryIdx(usize);

#[derive(Debug)]
pub struct CoreTypeIdx(usize);
