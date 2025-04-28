use crate::component_model::{
    CoreFunc, CoreGlobalRef, CoreInstance, CoreMemoryRef, CoreModule, CoreTableRef, CoreType,
    GlobalIdx,
};

#[repr(u8)]
#[derive(Debug, PartialEq)]
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
    Func(GlobalIdx<CoreFunc>),
    Table(GlobalIdx<CoreTableRef>),
    Memory(GlobalIdx<CoreMemoryRef>),
    Global(GlobalIdx<CoreGlobalRef>),
    Type(CoreType),
    Module(GlobalIdx<CoreModule>),
    Instance(GlobalIdx<CoreInstance>),
}
