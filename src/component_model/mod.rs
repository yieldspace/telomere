mod canon;
mod core;
mod idx;
mod types;

use crate::Module;
pub use canon::*;
pub use core::*;
pub use idx::*;
use std::collections::HashMap;
pub use types::*;

pub struct Component {
    children: Vec<Component>,
    pub imports: HashMap<String, ComponentImport>,
    pub exports: HashMap<String, ComponentExport>,

    pub core_modules: Vec<Module>,
    pub core_instances: Vec<CoreInstance>,
    pub core_functions: Vec<CoreFunction>,
    pub core_types: Vec<CoreType>,
}

impl Component {
    pub fn get_core_function(&self, idx: usize) -> &CoreFunction {
        self.core_functions
            .get(idx)
            .expect("Core function not found")
    }
}

pub enum ComponentImport {
    Instance(usize),
}

pub enum ComponentExport {
    Instance,
}
