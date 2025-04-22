use crate::component_model::{
    CoreFuncIdx, CoreGlobalIdx, CoreInstanceIdx, CoreMemoryIdx, CoreModuleIdx, CoreTableIdx,
    CoreTypeIdx,
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

#[derive(Debug, Copy, Clone)]
pub enum CoreSortWithIdx {
    Func(CoreFuncIdx),
    Table(CoreTableIdx),
    Memory(CoreMemoryIdx),
    Global(CoreGlobalIdx),
    Type(CoreTypeIdx),
    Module(CoreModuleIdx),
    Instance(CoreInstanceIdx),
}
