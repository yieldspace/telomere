use crate::component_model::{
    ComponentType, CoreModule, ExternDesc, Func, GlobalIdx, Instance, SortWithIdx, Type,
};
use crate::runtime::component_model::instantiate::InstantiateInstr;
use std::collections::HashMap;

#[derive(Clone)]
pub struct InlineComponent {
    pub value: Option<InlineComponentValue>,
    pub ty: ComponentType,
}

#[derive(Clone)]
pub struct InlineComponentValue {
    instrs: Vec<InstantiateInstr>,
    imports: HashMap<String, ComponentImport>,
    exports: HashMap<String, ComponentExport>,
}

impl InlineComponentValue {
    pub fn new(
        instrs: Vec<InstantiateInstr>,
        imports: HashMap<String, ComponentImport>,
        exports: HashMap<String, ComponentExport>,
    ) -> Self {
        Self {
            instrs,
            imports,
            exports,
        }
    }
}

impl InlineComponent {
    pub(crate) fn new(value: Option<InlineComponentValue>, ty: ComponentType) -> Self {
        Self { value, ty }
    }
}

#[derive(Debug, Clone)]
pub enum ComponentImport {
    CoreModule(GlobalIdx<CoreModule>),
    Func(GlobalIdx<Func>),
    #[cfg(feature = "component-gated-feature-value-imports-exports")]
    Value,
    Type(Type),
    Component(GlobalIdx<InlineComponent>),
    Instance(GlobalIdx<Instance>),
}

#[derive(Debug, Clone)]
pub struct ComponentExport {
    pub sort: SortWithIdx,
    pub desc: Option<ExternDesc>,
}
