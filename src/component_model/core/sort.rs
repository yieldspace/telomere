use crate::component_model::{
    CoreFuncIdx, CoreInstanceIdx, CoreMemoryIdx, CoreModuleIdx, CoreTypeIdx,
};

#[repr(u8)]
#[derive(Debug)]
pub enum CoreSort {
    Func = 0x00,
    Table = 0x01,
    Memory = 0x02,
    Global = 0x03,
    Type = 0x10,
    Module = 0x11,
    Instance = 0x12,
}

#[derive(Debug)]
pub enum CoreSortWithIdx {
    Func(CoreFuncIdx),
    Table(usize),
    Memory(CoreMemoryIdx),
    Global(usize),
    Type(CoreTypeIdx),
    Module(CoreModuleIdx),
    Instance(CoreInstanceIdx),
}
