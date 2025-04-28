use crate::component_model::{
    ComponentFunction, ComponentIdx, ComponentType, CoreModule, CoreModuleIdx, CoreSort,
    ExternDesc, FuncIdx, Instance, InstanceIdx, Slot, Sort, SortLike, SortWithIdx, Type, TypeIdx,
};
use crate::runtime::component_model::instantiate::InstantiateInstr;
use std::collections::HashMap;

/// コンポーネントからexportされた型を表します．
/// exportされた型に明示的にexterndescが設定されている場合，externdescの型として表されます．
#[allow(clippy::large_enum_variant)]
pub enum ComponentExportSlot {
    CoreModule(Slot<CoreModule, CoreModuleIdx>),
    Func(Slot<ComponentFunction, FuncIdx>),
    #[cfg(feature = "component-gated-feature-value-imports-exports")]
    Value,
    Type(Slot<Type, TypeIdx>),
    Component(Slot<InlineComponent, ComponentIdx>),
    Instance(Slot<Instance, InstanceIdx>),
}

impl SortLike for ComponentExportSlot {
    fn eq_sort(&self, sort: Sort) -> bool {
        match self {
            ComponentExportSlot::CoreModule(_) => sort == Sort::Core(CoreSort::Module),
            ComponentExportSlot::Func(_) => sort == Sort::Func,
            #[cfg(feature = "component-gated-feature-value-imports-exports")]
            ComponentExportSlot::Value => sort == Sort::Value,
            ComponentExportSlot::Type(_) => sort == Sort::Type,
            ComponentExportSlot::Component(_) => sort == Sort::Component,
            ComponentExportSlot::Instance(_) => sort == Sort::Instance,
        }
    }
}

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
    CoreModule(CoreModuleIdx),
    Func(FuncIdx),
    #[cfg(feature = "component-gated-feature-value-imports-exports")]
    Value,
    Type(TypeIdx),
    Component(ComponentIdx),
    Instance(InstanceIdx),
}

#[derive(Debug, Clone)]
pub struct ComponentExport {
    pub sort: SortWithIdx,
    pub desc: Option<ExternDesc>,
}
