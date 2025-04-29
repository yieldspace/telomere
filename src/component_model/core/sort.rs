use crate::component_model::{
    CoreFunc, CoreFuncType, CoreGlobalRef, CoreInstance, CoreInstanceType, CoreMemoryRef,
    CoreModule, CoreModuleType, CoreTableRef, CoreType, GlobalIdx,
};

#[repr(u8)]
#[derive(Debug, Clone, PartialEq)]
pub enum CoreSort {
    Func = 0x00,
    Table = 0x01,
    Memory = 0x02,
    Global = 0x03,
    Type = 0x10,
    Module = 0x11,
    Instance = 0x12,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CoreSortWithIdx {
    Func(GlobalIdx<CoreFunc>, CoreFuncType),
    Table(GlobalIdx<CoreTableRef>, crate::common::TableType),
    Memory(GlobalIdx<CoreMemoryRef>, crate::common::MemType),
    Global(GlobalIdx<CoreGlobalRef>, crate::common::GlobalType),
    Type(CoreType),
    Module(GlobalIdx<CoreModule>, CoreModuleType),
    Instance(GlobalIdx<CoreInstance>, CoreInstanceType),
}
