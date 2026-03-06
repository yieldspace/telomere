use crate::component::ir::types::{CoreFuncType, CoreType};
use crate::component::ir::{CanonicalOptions, Func, GlobalIdx, TypeId};
use crate::Module;
use std::collections::HashMap;

#[derive(Clone)]
pub struct CoreModule {
    pub module: Module,
}

impl std::fmt::Debug for CoreModule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CoreModule").finish_non_exhaustive()
    }
}

#[derive(Clone, Debug)]
pub enum CoreFunc {
    CanonLower {
        func: GlobalIdx<Func>,
        type_id: TypeId,
        options: Box<CanonicalOptions>,
        signature: CoreFuncType,
    },
    CanonResourceNew {
        type_id: TypeId,
    },
    CanonResourceDrop {
        type_id: TypeId,
    },
    CanonResourceRep {
        type_id: TypeId,
    },
}

#[derive(Clone, Debug)]
pub enum CoreInstance {
    Defined {
        module_idx: GlobalIdx<CoreModule>,
        imports: HashMap<String, GlobalIdx<CoreInstance>>,
    },
    InlineExport {
        exports: HashMap<String, CoreInstanceInlineExport>,
    },
}

#[derive(Clone, Debug)]
pub enum CoreInstanceInlineExport {
    Func(GlobalIdx<CoreFunc>),
    Memory(GlobalIdx<CoreMemory>),
    Global(GlobalIdx<CoreGlobal>),
    Table(GlobalIdx<CoreTable>),
    Type(GlobalIdx<CoreType>),
    Instance(GlobalIdx<CoreInstance>),
    Module(GlobalIdx<CoreModule>),
}

#[derive(Clone, Debug)]
pub struct CoreMemory(pub String);

#[derive(Clone, Debug)]
pub struct CoreGlobal(pub String);

#[derive(Clone, Debug)]
pub struct CoreTable(pub String);
