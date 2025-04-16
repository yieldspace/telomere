mod canon;
mod core;
mod func;
mod idx;
mod types;

use crate::binary::BinaryReader;
use crate::parser::component_model::{ComponentParseError, ParseContext, Validator};
use crate::runtime::component_model::instantiate::InstantiateInstr;
use crate::Module;
pub use canon::*;
pub use core::*;
pub use func::*;
pub use idx::*;
use std::cmp::PartialEq;
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

pub enum Binding<T> {
    Real(T),
    Alias(usize),
}

pub struct FlattenComponent {
    pub core_modules: Vec<Binding<Module>>,
    pub core_instances: Vec<Binding<CoreInstance>>,
    pub core_functions: Vec<Binding<CoreFunction>>,
    // pub core_types: Vec<CoreType>,
    pub functions: Vec<Binding<ComponentFunction>>,
    pub components: Vec<Binding<Component>>,
    pub instances: Vec<Binding<Instance>>,
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

    pub(crate) fn get_core_module(&self, idx: usize) -> &Module {
        match self
            .core_modules
            .get(idx)
            .expect("Core Module is not found")
        {
            Binding::Real(real) => real,
            Binding::Alias(idx) => self.get_core_module(*idx),
        }
    }

    pub(crate) fn get_core_instance(&self, idx: usize) -> &CoreInstance {
        match self
            .core_instances
            .get(idx)
            .expect("Core Instance is not found")
        {
            Binding::Real(real) => real,
            Binding::Alias(idx) => self.get_core_instance(*idx),
        }
    }

    pub fn get_core_function(&self, idx: usize) -> &CoreFunction {
        match self
            .core_functions
            .get(idx)
            .expect("Core function not found")
        {
            Binding::Real(real) => real,
            Binding::Alias(idx) => self.get_core_function(*idx),
        }
    }

    pub fn get_function(&self, idx: usize) -> &ComponentFunction {
        match self
            .functions
            .get(idx)
            .expect("Component function not found")
        {
            Binding::Real(real) => real,
            Binding::Alias(idx) => self.get_function(*idx),
        }
    }

    pub fn get_type(&self, idx: usize) -> &Type {
        todo!()
    }

    pub fn get_instance(&self, idx: usize) -> &Instance {
        todo!()
    }

    pub fn get_component(&self, idx: usize) -> &Component {
        match self.components.get(idx).expect("Component not found") {
            Binding::Real(real) => real,
            Binding::Alias(idx) => self.get_component(*idx),
        }
    }
}

pub enum ComponentImport {
    Instance(usize),
}

pub struct ComponentExport {
    name: String,
    sort: SortWithIdx,
    desc: ExternDesc,
}

#[derive(Debug)]
pub enum Instance {
    Instantiate(Instantiate),
    InlineExport(Vec<InlineExport>),
}

impl Instance {
    pub fn get_export(
        &self,
        ctx: &ParseContext<impl BinaryReader, impl Validator>,
        name: String,
        sort: Sort,
    ) -> Result<SortWithIdx, ComponentParseError> {
        match self {
            Instance::Instantiate(Instantiate { component_idx, .. }) => {
                let component = ctx.validator.get_component(component_idx);
                for export in &component.exports {
                    if export.name == name && export.sort.eq_sort(&sort) {
                        return Ok(export.sort.clone());
                    }
                }
            }
            Instance::InlineExport(exports) => {
                for export in exports {
                    if export.sort.eq_sort(&sort) && export.name == name {
                        return Ok(export.sort.clone());
                    }
                }
            }
        }
        todo!()
    }
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

#[derive(Debug, Clone)]
pub enum SortWithIdx {
    Core(CoreSortWithIdx),
    Func(FuncIdx),
    #[cfg(feature = "value")]
    Value(usize),
    Type(TypeIdx),
    Component(ComponentIdx),
    Instance(InstanceIdx),
}

impl SortWithIdx {
    pub(crate) fn eq_sort(&self, sort: &Sort) -> bool {
        match self {
            SortWithIdx::Core(cs) => match sort {
                Sort::Core(CoreSort::Func) => sort == &Sort::Core(CoreSort::Func),
                Sort::Core(CoreSort::Table) => sort == &Sort::Core(CoreSort::Table),
                Sort::Core(CoreSort::Memory) => sort == &Sort::Core(CoreSort::Memory),
                Sort::Core(CoreSort::Global) => sort == &Sort::Core(CoreSort::Global),
                Sort::Core(CoreSort::Type) => sort == &Sort::Core(CoreSort::Type),
                Sort::Core(CoreSort::Module) => sort == &Sort::Core(CoreSort::Module),
                Sort::Core(CoreSort::Instance) => sort == &Sort::Core(CoreSort::Instance),
                _ => false,
            },
            SortWithIdx::Func(_) => sort == &Sort::Func,
            #[cfg(feature = "component-value")]
            SortWithIdx::Value(_) => sort == &Sort::Value,
            SortWithIdx::Type(_) => sort == &Sort::Type,
            SortWithIdx::Component(_) => sort == &Sort::Component,
            SortWithIdx::Instance(_) => sort == &Sort::Instance,
        }
    }
}

#[derive(Debug, PartialEq)]
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
