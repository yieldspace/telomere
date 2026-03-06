use crate::component::ir::types::CoreFuncType;
use crate::component::ir::{CoreFunc, CoreMemory, GlobalIdx, TypeId};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CanonicalStringEncoding {
    Utf8,
    Utf16,
    CompactUtf16,
}

#[derive(Clone, Debug, Default)]
pub struct CanonicalOptions {
    pub string_encoding: Option<CanonicalStringEncoding>,
    pub memory: Option<GlobalIdx<CoreMemory>>,
    pub realloc: Option<GlobalIdx<CoreFunc>>,
    pub post_return: Option<GlobalIdx<CoreFunc>>,
    pub realloc_signature: Option<CoreFuncType>,
    pub post_return_signature: Option<CoreFuncType>,
}

#[derive(Clone, Debug)]
pub enum Func {
    CanonLift {
        core_func: GlobalIdx<CoreFunc>,
        type_id: TypeId,
        options: CanonicalOptions,
    },
}
