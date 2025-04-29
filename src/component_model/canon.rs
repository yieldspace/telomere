use crate::common::{ResultType as CoreResultType, ResultType, ValType as CoreValType};
use crate::component_model::flatten::Flattenable;
use crate::component_model::{
    CoreFunc, CoreFuncType, CoreMemoryRef, FuncType, GlobalIdx, ResourceType,
};
#[cfg(feature = "component-gated-feature-async")]
use crate::component_model::{CoreTableIdx, ValType};

#[derive(Debug)]
pub enum CanonicalFuncKind {
    #[cfg(feature = "component-gated-feature-async")]
    ResourceDropAsync(TypeIdx),
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
    ThreadSpawnIndirect(Type, GlobalIdx<CoreFunc>),
    #[cfg(feature = "component-gated-feature-threading-builtins")]
    ThreadAvailableParallelism,
}

#[derive(Debug, Clone)]
pub enum CanonOpt {
    StringEncodingUtf8,
    StringEncodingUtf16,
    StringEncodingLatin1Utf16,
    Memory(GlobalIdx<CoreMemoryRef>),
    Realloc(GlobalIdx<CoreFunc>),
    PostReturn(GlobalIdx<CoreFunc>),
    #[cfg(feature = "component-gated-feature-async")]
    Async,
    #[cfg(feature = "component-gated-feature-async")]
    Callback(GlobalIdx<CoreFunc>),
    #[cfg(feature = "component-gated-feature-async")]
    AlwaysTaskReturn,
}

pub trait CanonicalFuncType {
    fn canon_lower(ty: FuncType) -> Self;
    fn canon_resource_new(ty: ResourceType) -> Self;
    fn canon_resource_drop(ty: ResourceType) -> Self;
    fn canon_resource_rep(ty: ResourceType) -> Self;
}

impl CanonicalFuncType for CoreFuncType {
    fn canon_lower(ty: FuncType) -> Self {
        let FuncType { params, result } = ty;
        let params = params
            .into_iter()
            .map(|param| param.t.flat())
            .flatten()
            .collect::<Vec<_>>();
        let result = result.map(|x| x.flat()).unwrap_or_default();
        Self(ResultType(params), ResultType(result))
    }

    fn canon_resource_new(ty: ResourceType) -> Self {
        Self(
            CoreResultType(vec![CoreValType::I32]),
            CoreResultType(vec![CoreValType::I32]),
        )
    }

    fn canon_resource_drop(ty: ResourceType) -> Self {
        Self(
            CoreResultType(vec![CoreValType::I32]),
            CoreResultType(vec![CoreValType::I32]),
        )
    }

    fn canon_resource_rep(ty: ResourceType) -> Self {
        Self(
            CoreResultType(vec![CoreValType::I32]),
            CoreResultType(vec![CoreValType::I32]),
        )
    }
}
