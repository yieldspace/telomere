use crate::component_model::{CanonOpt, Component, CoreFuncIdx, FuncIdx, Idx, TypeIdx};
use crate::Store;

pub enum CoreFunction {
    Export(),
    CanonLower(FuncIdx, Vec<CanonOpt>),
    ResourceNew(TypeIdx),
    ResourceDrop(TypeIdx),
}

impl CoreFunction {
    pub fn instantiate(&self, store: &mut Store, component: &Component) {
        match self {
            CoreFunction::Export() => {}
            CoreFunction::CanonLower(idx, opts) => {
                let func = idx.get(component);
            }
            CoreFunction::ResourceNew(_) => {}
            CoreFunction::ResourceDrop(_) => {}
        }
    }
}
