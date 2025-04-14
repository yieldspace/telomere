use crate::component_model::{Component, CoreFunction};
use crate::component_model::func::ComponentFunction;

pub trait Idx<T> {
    fn get<'a>(&self, component: &'a Component) -> &'a T;
}

#[derive(Debug)]
pub struct TypeIdx(usize);

#[derive(Debug)]
pub struct CoreFuncIdx(usize);

#[derive(Debug)]
pub struct FuncIdx(usize);

impl Idx<ComponentFunction> for FuncIdx {
    fn get<'a>(&self, component: &'a Component) -> &'a ComponentFunction {
        component.get_function(self.0)
    }
}

impl Idx<CoreFunction> for CoreFuncIdx {
    fn get<'a>(&self, component: &'a Component) -> &'a CoreFunction {
        component.get_core_function(self.0)
    }
}

#[derive(Debug)]
pub struct CoreMemoryIdx(usize);

#[derive(Debug)]
pub struct CoreTypeIdx(usize);
