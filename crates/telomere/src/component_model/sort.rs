use crate::component_model::idx::GlobalIdx;
use crate::component_model::types::{
    CoreFuncType, CoreGlobalType, CoreInstanceType, CoreMemoryType, CoreModuleType, CoreTableType,
    CoreType,
};
use crate::component_model::{
    Component, CoreFunc, CoreGlobal, CoreInstance, CoreMemory, CoreModule, CoreTable, Func,
    Instance, TypeId,
};

#[derive(Debug, Clone, PartialEq)]
pub enum CoreSort {
    Module(GlobalIdx<CoreModule>, CoreModuleType),
    Instance(GlobalIdx<CoreInstance>, CoreInstanceType),
    Func(GlobalIdx<CoreFunc>, CoreFuncType),
    Table(GlobalIdx<CoreTable>, CoreTableType),
    Global(GlobalIdx<CoreGlobal>, CoreGlobalType),
    Memory(GlobalIdx<CoreMemory>, CoreMemoryType),
    Type(CoreType),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Sort {
    Core(CoreSort),
    Component(GlobalIdx<Component>, TypeId),
    Instance(GlobalIdx<Instance>, TypeId),
    Func(GlobalIdx<Func>, TypeId),
    Type(TypeId),
}
impl Sort {
    pub(crate) fn type_id(&self) -> TypeId {
        match self {
            Sort::Component(_global_idx, type_id) => *type_id,
            Sort::Instance(_global_idx, type_id) => *type_id,
            Sort::Func(_global_idx, type_id) => *type_id,
            Sort::Type(type_id) => *type_id,
            Sort::Core(_) => panic!("Core sorts do not have a type ID"), // FIXME: handle this case properly
        }
    }
}
