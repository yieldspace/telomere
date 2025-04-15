mod func;
mod sort;
mod types;

use crate::component_model::{
    CoreFuncIdx, CoreInstanceIdx, CoreModuleIdx,
};
pub use func::*;
pub use sort::*;
use std::collections::HashMap;
pub use types::*;

pub enum CoreInstance {
    Real {
        module_idx: CoreModuleIdx,
        imports: HashMap<String, CoreInstanceImport>,
    },
    Alias {
        exports: HashMap<String, CoreInstanceInlineExport>,
    },
}

pub enum CoreInstanceInlineExport {
    Func(CoreFuncIdx),
    Table(usize),
    Memory(usize),
    Global(usize),
}

pub enum CoreInstanceImport {
    Instance(CoreInstanceIdx),
}
