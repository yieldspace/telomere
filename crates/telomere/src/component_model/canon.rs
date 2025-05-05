use crate::component_model::{CoreFunc, CoreMemoryRef, GlobalIdx};
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

#[derive(Clone, Debug)]
pub enum CanonStringEncoding {
    Utf8,
    Utf16,
    Latin1Utf16,
}

#[derive(Clone, Debug)]
pub struct CanonicalOptions {
    pub(crate) string_encoding: CanonStringEncoding,
    pub(crate) memory: Option<GlobalIdx<CoreMemoryRef>>,
    pub(crate) realloc: Option<GlobalIdx<CoreFunc>>,
    pub(crate) post_return: Option<GlobalIdx<CoreFunc>>,
    #[cfg(feature = "component-gated-feature-async")]
    pub(crate) is_async: bool,
    #[cfg(feature = "component-gated-feature-async")]
    pub(crate) callback: Option<GlobalIdx<CoreFunc>>,
    #[cfg(feature = "component-gated-feature-async")]
    pub(crate) always_task_return: bool,
}

impl CanonicalOptions {
    #[cfg(not(feature = "component-gated-feature-async"))]
    pub fn is_sync(&self) -> bool {
        true
    }

    #[cfg(feature = "component-gated-feature-async")]
    pub fn is_sync(&self) -> bool {
        !self.is_async
    }
}

impl From<Vec<CanonOpt>> for CanonicalOptions {
    fn from(value: Vec<CanonOpt>) -> Self {
        let mut string_encoding = CanonStringEncoding::Utf8;
        let mut memory = None;
        let mut realloc = None;
        let mut post_return = None;
        #[cfg(feature = "component-gated-feature-async")]
        let mut is_async = false;
        #[cfg(feature = "component-gated-feature-async")]
        let mut callback = None;
        #[cfg(feature = "component-gated-feature-async")]
        let mut always_task_return = false;

        for opt in value {
            match opt {
                CanonOpt::StringEncodingUtf8 => string_encoding = CanonStringEncoding::Utf8,
                CanonOpt::StringEncodingUtf16 => string_encoding = CanonStringEncoding::Utf16,
                CanonOpt::StringEncodingLatin1Utf16 => {
                    string_encoding = CanonStringEncoding::Latin1Utf16
                }
                CanonOpt::Memory(m) => memory = Some(m),
                CanonOpt::Realloc(r) => realloc = Some(r),
                CanonOpt::PostReturn(p) => post_return = Some(p),
                #[cfg(feature = "component-gated-feature-async")]
                CanonOpt::Async => is_async = true,
                #[cfg(feature = "component-gated-feature-async")]
                CanonOpt::Callback(c) => callback = Some(c),
                #[cfg(feature = "component-gated-feature-async")]
                CanonOpt::AlwaysTaskReturn => always_task_return = true,
            }
        }

        Self {
            string_encoding,
            memory,
            realloc,
            post_return,
            #[cfg(feature = "component-gated-feature-async")]
            is_async,
            #[cfg(feature = "component-gated-feature-async")]
            callback,
            #[cfg(feature = "component-gated-feature-async")]
            always_task_return,
        }
    }
}
