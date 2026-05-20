use crate::ir::types::{CoreFuncType, CoreType, ValType};
use crate::ir::{CanonicalOptions, Func, GlobalIdx, TypeId};
use crate::support::Module;
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
    CanonTaskCancel,
    CanonSubtaskCancel {
        async_: bool,
    },
    CanonSubtaskDrop,
    CanonErrorContextNew {
        options: Box<CanonicalOptions>,
    },
    CanonErrorContextDebugMessage {
        options: Box<CanonicalOptions>,
    },
    CanonErrorContextDrop,
    CanonWaitableSetNew,
    CanonWaitableSetWait {
        cancellable: bool,
        memory: GlobalIdx<CoreMemory>,
    },
    CanonWaitableSetPoll {
        cancellable: bool,
        memory: GlobalIdx<CoreMemory>,
    },
    CanonWaitableSetDrop,
    CanonWaitableJoin,
    CanonTaskReturn {
        result: Option<ValType>,
        options: Box<CanonicalOptions>,
        signature: CoreFuncType,
    },
    CanonStreamNew {
        type_id: TypeId,
    },
    CanonStreamRead {
        type_id: TypeId,
        options: Box<CanonicalOptions>,
    },
    CanonStreamWrite {
        type_id: TypeId,
        options: Box<CanonicalOptions>,
    },
    CanonStreamCancelRead {
        type_id: TypeId,
        async_: bool,
    },
    CanonStreamCancelWrite {
        type_id: TypeId,
        async_: bool,
    },
    CanonStreamDropReadable {
        type_id: TypeId,
    },
    CanonStreamDropWritable {
        type_id: TypeId,
    },
    CanonFutureNew {
        type_id: TypeId,
    },
    CanonFutureRead {
        type_id: TypeId,
        options: Box<CanonicalOptions>,
    },
    CanonFutureWrite {
        type_id: TypeId,
        options: Box<CanonicalOptions>,
    },
    CanonFutureCancelRead {
        type_id: TypeId,
        async_: bool,
    },
    CanonFutureCancelWrite {
        type_id: TypeId,
        async_: bool,
    },
    CanonFutureDropReadable {
        type_id: TypeId,
    },
    CanonFutureDropWritable {
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
