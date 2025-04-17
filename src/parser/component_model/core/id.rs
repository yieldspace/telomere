use crate::binary::BinaryReader;
use crate::component_model::{
    CoreFuncIdx, CoreInstanceIdx, CoreMemoryIdx, CoreModuleIdx, CoreTypeIdx,
};
use crate::parser::component_model::context::ParseContext;
use crate::parser::component_model::validator::Validator;
use crate::parser::component_model::SizedResult;
use crate::parser::core::parse_u32;

pub fn parse_core_module_idx(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> SizedResult<CoreModuleIdx> {
    let (len, idx) = parse_u32(ctx.reader)?;
    Ok((len, ctx.validator.validate_core_module_idx(idx as usize)?))
}

pub fn parse_core_instance_idx(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> SizedResult<CoreInstanceIdx> {
    let (len, idx) = parse_u32(ctx.reader)?;
    Ok((len, ctx.validator.validate_core_instance_idx(idx as usize)?))
}

pub fn parse_core_func_idx(ctx: &mut ParseContext<impl BinaryReader>) -> SizedResult<CoreFuncIdx> {
    let (len, idx) = parse_u32(ctx.reader)?;
    Ok((len, ctx.validator.validate_core_function_idx(idx as usize)?))
}

pub fn parse_core_memory_idx(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> SizedResult<CoreMemoryIdx> {
    let (len, idx) = parse_u32(ctx.reader)?;
    Ok((len, ctx.validator.validate_core_memory_idx(idx as usize)?))
}

pub fn parse_core_type_idx(ctx: &mut ParseContext<impl BinaryReader>) -> SizedResult<CoreTypeIdx> {
    let (len, idx) = parse_u32(ctx.reader)?;
    Ok((len, ctx.validator.validate_core_type_idx(idx as usize)?))
}
