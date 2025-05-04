use crate::component_model::{
    ComponentType, CoreModule, CoreModuleType, ExportName, Func, FuncType, GlobalIdx, ImportName,
    Instance, InstanceType, Type,
};
use crate::runtime::component_model::instantiate::InstantiateInstr;
use std::collections::HashMap;

#[derive(Clone)]
pub struct InlineComponent {
    pub(crate) instrs: Vec<InstantiateInstr>,
    pub(crate) imports: HashMap<ImportName, ComponentImport>,
    pub(crate) exports: HashMap<ExportName, ComponentExport>,
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
