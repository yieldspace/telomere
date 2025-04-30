use crate::Module;

#[derive(Clone)]
pub struct CoreModule {
    pub value: Module,
}

impl CoreModule {
    pub fn new(value: Module) -> Self {
        Self { value }
    }
}
