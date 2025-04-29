use crate::component_model::{
    ComponentType, CoreFuncType, CoreModule, CoreModuleType, ExternDesc, Func, FuncType, GlobalIdx,
    Instance, InstanceType, SortWithIdx, Type,
};
use crate::runtime::component_model::instantiate::InstantiateInstr;
use std::collections::HashMap;

#[derive(Clone)]
pub struct InlineComponent {
    pub(crate) instrs: Vec<InstantiateInstr>,
    pub(crate) imports: HashMap<String, ComponentImport>,
    pub(crate) exports: HashMap<String, ComponentExport>,
}
#[derive(Debug, Clone)]
pub enum ComponentImport {
    CoreModule(CoreModuleType, GlobalIdx<CoreModule>),
    Func(FuncType, GlobalIdx<Func>),
    #[cfg(feature = "component-gated-feature-value-imports-exports")]
    Value,
    Type(Type),
    Component(ComponentType, GlobalIdx<InlineComponent>),
    Instance(InstanceType, GlobalIdx<Instance>),
}

#[derive(Debug, Clone)]
pub enum ComponentExport {
    CoreModule(CoreModuleType, GlobalIdx<CoreModule>),
    Func(FuncType, GlobalIdx<Func>),
    #[cfg(feature = "component-gated-feature-value-imports-exports")]
    Value,
    Type(Type),
    Component(ComponentType, GlobalIdx<InlineComponent>),
    Instance(InstanceType, GlobalIdx<Instance>),
}
