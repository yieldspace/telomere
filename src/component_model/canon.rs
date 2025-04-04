use crate::component_model::id::{CoreFuncId, CoreMemoryId, CoreTableId, FuncId, TypeId};
use crate::component_model::types::ValType;

#[derive(Debug)]
pub enum CanonicalFuncKind {
    CanonLift(FuncId, Vec<CanonOpt>, TypeId),
    CanonLower(FuncId, Vec<CanonOpt>),
    ResourceNew(TypeId),
    ResourceDrop(TypeId),
    ResourceDropAsync(TypeId),
    ResourceRep(TypeId),
    BackPressureSet,
    TaskReturn(Option<ValType>, Vec<CanonOpt>),
    ContextGet(u32),
    ContextSet(u32),
    YieldAsync(Option<bool>),
    SubtaskDrop,
    StreamNew(TypeId),
    StreamRead(TypeId, Vec<CanonOpt>),
    StreamWrite(TypeId, Vec<CanonOpt>),
    StreamCancelRead(TypeId, Option<bool>),
    StreamCancelWrite(TypeId, Option<bool>),
    StreamCloseReadable(TypeId),
    StreamCloseWritable(TypeId),
    FutureNew(TypeId),
    FutureRead(TypeId, Vec<CanonOpt>),
    FutureWrite(TypeId, Vec<CanonOpt>),
    FutureCancelRead(TypeId, Option<bool>),
    FutureCancelWrite(TypeId, Option<bool>),
    FutureCloseReadable(TypeId),
    FutureCloseWritable(TypeId),
    ErrorContextNew(Vec<CanonOpt>),
    ErrorContextDebugMessage(Vec<CanonOpt>),
    ErrorContextDrop,
    WaitableSetNew,
    WaitableSetWait(Option<bool>, CoreMemoryId),
    WaitableSetPoll(Option<bool>, CoreMemoryId),
    WaitableSetDrop,
    WaitableJoin,
    ThreadSpawnRef(TypeId),
    ThreadSpawnIndirect(TypeId, CoreTableId),
    ThreadAvailableParallelism,
}

#[derive(Debug)]
pub enum CanonOpt {
    StringEncodingUtf8,
    StringEncodingUtf16,
    StringEncodingLatin1Utf16,
    Memory(CoreMemoryId),
    Realloc(CoreFuncId),
    PostReturn(CoreFuncId),
    #[cfg(feature = "async")]
    Async,
    #[cfg(feature = "async")]
    Callback(CoreFuncId),
    #[cfg(feature = "async")]
    AlwaysTaskReturn,
}
