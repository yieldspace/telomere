mod func;
mod sort;
mod types;

use crate::component_model::{
    CanonOpt, CanonicalFuncKind, CoreFuncIdx, CoreInstanceIdx, CoreModuleIdx, FlattenComponent,
    Idx, TypeIdx,
};
use crate::runtime::component_model::{ComponentInstantiated, CoreInstantiated, Linker};
use crate::{Registry, Store};
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
