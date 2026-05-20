mod flatten;
mod options;
mod parse;

use crate::decoder::types::parse_valtype;
use crate::decoder::{
    parse_core_func_local_idx, parse_core_memory_local_idx, parse_core_type_local_idx,
    parse_func_local_idx, parse_type_local_idx, parse_vec_range, ComponentParseError, ParseContext,
    ParseResult,
};
use crate::ir::types::{DefValType, FuncType, PrimValType, Type, ValType};
use crate::ir::{
    CanonicalOptions, CanonicalStringEncoding, CoreFunc, CoreRelation, Func, Relation, TypeId,
};
use crate::support::binary::BinaryReader;
use crate::support::common::{FuncType as CoreFuncType, ValType as CoreValType};

const MAX_FLAT_PARAMS: usize = 16;
const MAX_FLAT_RESULTS: usize = 1;

#[derive(Clone, Copy)]
enum ResourceCanonKind {
    New,
    Drop,
    Rep,
}

#[derive(Clone, Copy)]
enum CanonMode {
    Lift,
    Lower,
}

pub use parse::parse_canon;
