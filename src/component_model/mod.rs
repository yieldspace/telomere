mod canon;
mod core;
mod func;
mod idx;
mod types;

use crate::component_model::func::ComponentFunction;
use crate::runtime::component_model::instantiate::InstantiateInstr;
use crate::Module;
pub use canon::*;
pub use core::*;
pub use idx::*;
pub use types::*;

#[derive(Copy, Clone, Hash, PartialEq, Eq, Debug)]
pub struct ComponentId(pub usize);

pub struct Component {
    pub instrs: Vec<InstantiateInstr>,
    pub imports: Vec<ComponentImport>,
    pub exports: Vec<ComponentExport>,
}

impl Component {
    pub(crate) fn new(instrs: Vec<InstantiateInstr>) -> Self {
        Self {
            instrs,
            imports: vec![],
            exports: vec![],
        }
    }
}

pub struct FlattenComponent {
    pub core_modules: Vec<Module>,
    pub core_instances: Vec<CoreInstance>,
    pub core_functions: Vec<CoreFunction>,
    // pub core_types: Vec<CoreType>,
    pub functions: Vec<ComponentFunction>,
    pub components: Vec<Component>,
    pub instances: Vec<Instance>,
}

impl FlattenComponent {
    pub fn new() -> Self {
        FlattenComponent {
            core_modules: Vec::new(),
            core_instances: Vec::new(),
            core_functions: Vec::new(),
            functions: Vec::new(),
            components: vec![],
            instances: Vec::new(),
        }
    }

    pub fn get_core_function(&self, idx: usize) -> &CoreFunction {
        self.core_functions
            .get(idx)
            .expect("Core function not found")
    }

    pub fn get_function(&self, idx: usize) -> &ComponentFunction {
        self.functions
            .get(idx)
            .expect("Component function not found")
    }
}

pub enum ComponentImport {
    Instance(usize),
}

pub enum ComponentExport {
    Instance,
}

#[derive(Debug)]
pub enum Instance {
    Instantiate(Instantiate),
    InlineExport(Vec<InlineExport>),
}

#[derive(Debug)]
pub struct Instantiate {
    pub component_idx: ComponentIdx,
    pub args: Vec<InstantiateArg>,
}

#[derive(Debug)]
pub struct InstantiateArg {
    pub name: String,
    pub sort: SortWithIdx,
}

#[derive(Debug)]
#[repr(u8)]
pub enum SortType {
    Core(CoreSort) = 0x00,
    Func = 0x01,
    Value = 0x02,
    Type = 0x03,
    Component = 0x04,
    Instance = 0x05,
}

#[derive(Debug)]
pub enum SortWithIdx {
    Core(CoreSortWithIdx),
    Func(FuncIdx),
    #[cfg(feature = "value")]
    Value(usize),
    Type(TypeIdx),
    Component(ComponentIdx),
    Instance(InstanceIdx),
}

#[derive(Debug)]
pub enum Sort {
    Core(CoreSort),
    Func,
    Value,
    Type,
    Component,
    Instance,
}

#[derive(Debug)]
pub struct InlineExport {
    pub name: String,
    pub sort: SortWithIdx,
}
