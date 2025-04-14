use super::idx::TypeIdx;
use crate::component_model::{CoreFuncIdx, CoreMemoryIdx};

#[derive(Debug)]
pub enum CanonicalFuncKind {
    CanonLift(CoreFuncIdx, Vec<CanonOpt>, TypeIdx),
    CanonLower(CoreFuncIdx, Vec<CanonOpt>),
    ResourceNew(TypeIdx),
    ResourceDrop(TypeIdx),
    // ResourceDropAsync(TypeIdx),
    // ResourceRep(TypeIdx),
    // BackPressureSet,
    // TaskReturn(Option<ValType>, Vec<CanonOpt>),
    // ContextGet(u32),
    // ContextSet(u32),
    // YieldAsync(Option<bool>),
    // SubtaskDrop,
    // StreamNew(TypeIdx),
    // StreamRead(TypeIdx, Vec<CanonOpt>),
    // StreamWrite(TypeIdx, Vec<CanonOpt>),
    // StreamCancelRead(TypeIdx, Option<bool>),
    // StreamCancelWrite(TypeIdx, Option<bool>),
    // StreamCloseReadable(TypeIdx),
    // StreamCloseWritable(TypeIdx),
    // FutureNew(TypeIdx),
    // FutureRead(TypeIdx, Vec<CanonOpt>),
    // FutureWrite(TypeIdx, Vec<CanonOpt>),
    // FutureCancelRead(TypeIdx, Option<bool>),
    // FutureCancelWrite(TypeIdx, Option<bool>),
    // FutureCloseReadable(TypeIdx),
    // FutureCloseWritable(TypeIdx),
    // ErrorContextNew(Vec<CanonOpt>),
    // ErrorContextDebugMessage(Vec<CanonOpt>),
    // ErrorContextDrop,
    // WaitableSetNew,
    // WaitableSetWait(Option<bool>, CoreMemoryId),
    // WaitableSetPoll(Option<bool>, CoreMemoryId),
    // WaitableSetDrop,
    // WaitableJoin,
    // ThreadSpawnRef(TypeIdx),
    // ThreadSpawnIndirect(TypeIdx, CoreTableId),
    // ThreadAvailableParallelism,
}

#[derive(Debug)]
pub enum CanonOpt {
    StringEncodingUtf8,
    StringEncodingUtf16,
    StringEncodingLatin1Utf16,
    Memory(CoreMemoryIdx),
    Realloc(CoreFuncIdx),
    PostReturn(CoreFuncIdx),
    // #[cfg(feature = "async")]
    // Async,
    // #[cfg(feature = "async")]
    // Callback(CoreFuncId),
    // #[cfg(feature = "async")]
    // AlwaysTaskReturn,
}
