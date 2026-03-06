#[derive(Debug, Clone, PartialEq)]
pub enum SortType {
    Core(CoreSortType),
    Component,
    Func,
    Type,
    Instance,
}

#[repr(u8)]
#[derive(Debug, Clone, PartialEq)]
pub enum CoreSortType {
    Func = 0x00,
    Table = 0x01,
    Memory = 0x02,
    Global = 0x03,
    Type = 0x10,
    Module = 0x11,
    Instance = 0x12,
}
