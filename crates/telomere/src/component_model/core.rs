use crate::component_model::types::CoreType;
use crate::component_model::GlobalIdx;
use crate::Module;
use std::collections::HashMap;

pub struct CoreModule {
    pub module: Module,
}

pub enum CoreFunc {
    Export(String),
    CanonLower,
}

pub enum CoreInstance {
    Defined {
        module_idx: GlobalIdx<CoreModule>,
        imports: HashMap<String, GlobalIdx<CoreInstance>>,
    },
    InlineExport {
        exports: HashMap<String, CoreInstanceInlineExport>,
    },
}

pub enum CoreInstanceInlineExport {
    Func(GlobalIdx<CoreFunc>),
    Memory(GlobalIdx<CoreMemory>),
    Global(GlobalIdx<CoreGlobal>),
    Table(GlobalIdx<CoreTable>),
    Type(GlobalIdx<CoreType>),
    Instance(GlobalIdx<CoreInstance>),
    Module(GlobalIdx<CoreModule>),
}

pub struct CoreMemory(pub String);
pub struct CoreGlobal(pub String);
pub struct CoreTable(pub String);
