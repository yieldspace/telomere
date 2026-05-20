use crate::ir::types::{CoreFuncType, CoreType};
use crate::ir::{CoreFunc, CoreMemory, GlobalIdx, TypeId};

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
    pub async_: bool,
    pub callback: Option<GlobalIdx<CoreFunc>>,
    pub core_type: Option<GlobalIdx<CoreType>>,
    pub gc: bool,
    pub realloc_signature: Option<CoreFuncType>,
    pub post_return_signature: Option<CoreFuncType>,
    pub callback_signature: Option<CoreFuncType>,
    pub core_type_signature: Option<CoreFuncType>,
}

#[derive(Clone, Debug)]
pub enum Func {
    CanonLift {
        core_func: GlobalIdx<CoreFunc>,
        type_id: TypeId,
        options: CanonicalOptions,
    },
}
