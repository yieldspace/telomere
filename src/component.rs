pub struct Component {}

pub enum CoreInstance {
    Instantiate(CoreInstantiate),
    InlineExport(Vec<CoreInstanceInlineExport>),
}

pub struct CoreInstantiate {
    pub module_idx: usize,
    pub args: Vec<CoreInstanceArg>,
}

pub struct CoreInstanceArg {
    pub name: String,
    pub instance_idx: usize,
}

pub struct CoreInstanceInlineExport {
    pub name: String,
    pub sort: CoreSort,
    pub sort_idx: usize,
}

#[repr(u8)]
pub enum CoreSort {
    Func = 0x00,
    Table = 0x01,
    Memory = 0x02,
    Global = 0x03,
    Type = 0x10,
    Module = 0x11,
    Instance = 0x12,
}
