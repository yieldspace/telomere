use crate::binary::BinaryReader;
use crate::component_model::{
    CoreFuncIdx, CoreGlobalIdx, CoreInstanceIdx, CoreMemoryIdx, CoreModuleIdx, CoreTableIdx,
    CoreTypeIdx,
};
use crate::parser::component_model::context::ParseContext;
use crate::parser::component_model::{SizedResult, Validator};
use crate::parser::core::parse_u32;

pub fn parse_core_module_idx(
    ctx: &mut ParseContext<impl BinaryReader, impl Validator>,
) -> SizedResult<CoreModuleIdx> {
    let (len, idx) = parse_u32(ctx.reader)?;
    Ok((len, ctx.validator.validate_core_module_idx(idx as usize)?))
}

pub fn parse_core_instance_idx(
    ctx: &mut ParseContext<impl BinaryReader, impl Validator>,
) -> SizedResult<CoreInstanceIdx> {
    let (len, idx) = parse_u32(ctx.reader)?;
    Ok((len, ctx.validator.validate_core_instance_idx(idx as usize)?))
}

pub fn parse_core_func_idx(ctx: &mut ParseContext<impl BinaryReader, impl Validator>) -> SizedResult<CoreFuncIdx> {
    let (len, idx) = parse_u32(ctx.reader)?;
    Ok((len, ctx.validator.validate_core_function_idx(idx as usize)?))
}

pub fn parse_core_memory_idx(
    ctx: &mut ParseContext<impl BinaryReader, impl Validator>,
) -> SizedResult<CoreMemoryIdx> {
    let (len, idx) = parse_u32(ctx.reader)?;
    Ok((len, ctx.validator.validate_core_memory_idx(idx as usize)?))
}

pub fn parse_core_type_idx(ctx: &mut ParseContext<impl BinaryReader, impl Validator>) -> SizedResult<CoreTypeIdx> {
    let (len, idx) = parse_u32(ctx.reader)?;
    Ok((len, ctx.validator.validate_core_type_idx(idx as usize)?))
}

pub fn parse_core_table_idx(
    ctx: &mut ParseContext<impl BinaryReader, impl Validator>,
) -> SizedResult<CoreTableIdx> {
    let (len, idx) = parse_u32(ctx.reader)?;
    Ok((len, ctx.validator.validate_core_table_idx(idx as usize)?))
}

pub fn parse_core_global_idx(
    ctx: &mut ParseContext<impl BinaryReader, impl Validator>,
) -> SizedResult<CoreGlobalIdx> {
    let (len, idx) = parse_u32(ctx.reader)?;
    Ok((len, ctx.validator.validate_core_global_idx(idx as usize)?))
}
