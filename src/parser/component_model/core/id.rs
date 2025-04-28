use crate::binary::BinaryReader;
use crate::component_model::{
    CoreFuncIdx, CoreGlobalIdx, CoreInstanceIdx, CoreMemoryIdx, CoreModuleIdx, CoreTableIdx,
    CoreType, CoreTypeIdx,
};
use crate::parser::component_model::context::ParseContext;
use crate::parser::component_model::{ComponentParseError, ParseResult, Validator};
use crate::parser::component_model::validator::LocalIdx;
use crate::parser::core::parse_u32;

pub fn parse_core_module_idx(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> ParseResult<LocalIdx> {
    let (len, idx) = parse_u32(ctx.reader)?;
    Ok(ctx.validator.validate_core_module_idx(idx)?)
}

pub fn parse_core_instance_idx(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> ParseResult<LocalIdx> {
    let (_, idx) = parse_u32(ctx.reader)?;
    Ok(ctx.validator.validate_core_instance_idx(idx)?)
}

pub fn parse_core_func_idx(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> ParseResult<LocalIdx> {
    let (_, idx) = parse_u32(ctx.reader)?;
    Ok(ctx.validator.validate_core_func_idx(idx)?)
}

pub fn parse_core_memory_idx(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> ParseResult<LocalIdx> {
    let (len, idx) = parse_u32(ctx.reader)?;
    Ok(ctx.validator.validate_core_memory_idx(idx)?)
}

pub fn parse_core_type_idx(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> ParseResult<LocalIdx> {
    let (len, idx) = parse_u32(ctx.reader)?;
    Ok(ctx.validator.validate_core_type_idx(idx)?)
}

pub fn parse_core_table_idx(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> ParseResult<LocalIdx> {
    let (len, idx) = parse_u32(ctx.reader)?;
    Ok(ctx.validator.validate_core_table_idx(idx)?)
}

pub fn parse_core_global_idx(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> ParseResult<LocalIdx> {
    let (len, idx) = parse_u32(ctx.reader)?;
    Ok(ctx.validator.validate_core_global_idx(idx)?)
}
