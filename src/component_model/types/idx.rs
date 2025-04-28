use crate::component_model::Idx;

#[derive(Clone)]
pub struct VirtualTypeIdx(usize);

impl Idx for VirtualTypeIdx {
    fn new(global: usize) -> Self {
        Self(global)
    }
    fn global(&self) -> usize {
        panic!("This idx has not global idx")
    }
}
