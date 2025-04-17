use super::idx::TypeIdx;
use crate::component_model::{CoreFuncIdx, CoreMemoryIdx, FuncIdx};
#[cfg(feature = "component-gated-feature-async")]
use crate::component_model::{CoreTableIdx, ValType};

#[derive(Debug)]
pub enum CanonicalFuncKind {
    CanonLift(CoreFuncIdx, Vec<CanonOpt>, TypeIdx),
    CanonLower(FuncIdx, Vec<CanonOpt>),
    ResourceNew(TypeIdx),
    ResourceDrop(TypeIdx),
    #[cfg(feature = "component-gated-feature-async")]
    ResourceDropAsync(TypeIdx),
    ResourceRep(TypeIdx),
    #[cfg(feature = "component-gated-feature-async")]
    BackPressureSet,
    #[cfg(feature = "component-gated-feature-async")]
    TaskReturn(Option<ValType>, Vec<CanonOpt>),
    #[cfg(feature = "component-gated-feature-async")]
    ContextGet(u32),
    #[cfg(feature = "component-gated-feature-async")]
    ContextSet(u32),
    #[cfg(feature = "component-gated-feature-async")]
    YieldAsync(Option<bool>),
    #[cfg(feature = "component-gated-feature-async")]
    SubtaskDrop,
    #[cfg(feature = "component-gated-feature-async")]
    StreamNew(TypeIdx),
    #[cfg(feature = "component-gated-feature-async")]
    StreamRead(TypeIdx, Vec<CanonOpt>),
    #[cfg(feature = "component-gated-feature-async")]
    StreamWrite(TypeIdx, Vec<CanonOpt>),
    #[cfg(feature = "component-gated-feature-async")]
    StreamCancelRead(TypeIdx, Option<bool>),
    #[cfg(feature = "component-gated-feature-async")]
    StreamCancelWrite(TypeIdx, Option<bool>),
    #[cfg(feature = "component-gated-feature-async")]
    StreamCloseReadable(TypeIdx),
    #[cfg(feature = "component-gated-feature-async")]
    StreamCloseWritable(TypeIdx),
    #[cfg(feature = "component-gated-feature-async")]
    FutureNew(TypeIdx),
    #[cfg(feature = "component-gated-feature-async")]
    FutureRead(TypeIdx, Vec<CanonOpt>),
    #[cfg(feature = "component-gated-feature-async")]
    FutureWrite(TypeIdx, Vec<CanonOpt>),
    #[cfg(feature = "component-gated-feature-async")]
    FutureCancelRead(TypeIdx, Option<bool>),
    #[cfg(feature = "component-gated-feature-async")]
    FutureCancelWrite(TypeIdx, Option<bool>),
    #[cfg(feature = "component-gated-feature-async")]
    FutureCloseReadable(TypeIdx),
    #[cfg(feature = "component-gated-feature-async")]
    FutureCloseWritable(TypeIdx),
    #[cfg(feature = "component-gated-feature-error-context-type")]
    ErrorContextNew(Vec<CanonOpt>),
    #[cfg(feature = "component-gated-feature-error-context-type")]
    ErrorContextDebugMessage(Vec<CanonOpt>),
    #[cfg(feature = "component-gated-feature-error-context-type")]
    ErrorContextDrop,
    #[cfg(feature = "component-gated-feature-async")]
    WaitableSetNew,
    #[cfg(feature = "component-gated-feature-async")]
    WaitableSetWait(Option<bool>, CoreMemoryIdx),
    #[cfg(feature = "component-gated-feature-async")]
    WaitableSetPoll(Option<bool>, CoreMemoryIdx),
    #[cfg(feature = "component-gated-feature-async")]
    WaitableSetDrop,
    #[cfg(feature = "component-gated-feature-async")]
    WaitableJoin,
    #[cfg(feature = "component-gated-feature-threading-builtins")]
    ThreadSpawnRef(TypeIdx),
    #[cfg(feature = "component-gated-feature-threading-builtins")]
    ThreadSpawnIndirect(TypeIdx, CoreTableIdx),
    #[cfg(feature = "component-gated-feature-threading-builtins")]
    ThreadAvailableParallelism,
}

#[derive(Debug)]
pub enum CanonOpt {
    StringEncodingUtf8,
    StringEncodingUtf16,
    StringEncodingLatin1Utf16,
    Memory(CoreMemoryIdx),
    Realloc(CoreFuncIdx),
    PostReturn(CoreFuncIdx),
    #[cfg(feature = "component-gated-feature-async")]
    Async,
    #[cfg(feature = "component-gated-feature-async")]
    Callback(CoreFuncIdx),
    #[cfg(feature = "component-gated-feature-async")]
    AlwaysTaskReturn,
}
