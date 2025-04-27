use crate::binary::BinaryReader;
use crate::component_model::{CoreFuncIdx, CoreGlobalIdx, CoreInstanceIdx, CoreMemoryIdx, CoreModuleIdx, CoreTableIdx, CoreType, CoreTypeIdx};
use crate::parser::component_model::context::ParseContext;
use crate::parser::component_model::{DefaultValidator, SizedResult, Validator};
use crate::parser::component_model::validator::IdxValidator;
use crate::parser::core::parse_u32;

pub fn parse_core_module_idx(
    ctx: &mut ParseContext<impl BinaryReader, impl DefaultValidator>,
) -> SizedResult<CoreModuleIdx> {
    let (len, idx) = parse_u32(ctx.reader)?;
    Ok((len, ctx.validator.validate_idx(idx)?))
}

pub fn parse_core_instance_idx(
    ctx: &mut ParseContext<impl BinaryReader, impl DefaultValidator>,
) -> SizedResult<CoreInstanceIdx> {
    let (len, idx) = parse_u32(ctx.reader)?;
    Ok((len, ctx.validator.validate_idx(idx)?))
}

pub fn parse_core_func_idx(
    ctx: &mut ParseContext<impl BinaryReader, impl DefaultValidator>,
) -> SizedResult<CoreFuncIdx> {
    let (len, idx) = parse_u32(ctx.reader)?;
    Ok((len, ctx.validator.validate_idx(idx)?))
}

pub fn parse_core_memory_idx(
    ctx: &mut ParseContext<impl BinaryReader, impl DefaultValidator>,
) -> SizedResult<CoreMemoryIdx> {
    let (len, idx) = parse_u32(ctx.reader)?;
    Ok((len, ctx.validator.validate_idx(idx)?))
}

pub fn parse_core_type_idx(
    ctx: &mut ParseContext<impl BinaryReader, impl DefaultValidator>,
) -> SizedResult<CoreTypeIdx> {
    let (len, idx) = parse_u32(ctx.reader)?;
    Ok((len, ctx.validator.validate_idx(idx)?))
}

pub fn parse_core_type_idx_resolved(
    ctx: &mut ParseContext<impl BinaryReader, impl Validator + IdxValidator<CoreTypeIdx, CoreType>>,
) -> SizedResult<CoreType> {
    let (len, idx) = parse_u32(ctx.reader)?;
    Ok((len, ctx.validator.validate_idx_resolved(idx)?))
}

pub fn parse_core_table_idx(
    ctx: &mut ParseContext<impl BinaryReader, impl DefaultValidator>,
) -> SizedResult<CoreTableIdx> {
    let (len, idx) = parse_u32(ctx.reader)?;
    Ok((len, ctx.validator.validate_idx(idx)?))
}

pub fn parse_core_global_idx(
    ctx: &mut ParseContext<impl BinaryReader, impl DefaultValidator>,
) -> SizedResult<CoreGlobalIdx> {
    let (len, idx) = parse_u32(ctx.reader)?;
    Ok((len, ctx.validator.validate_idx(idx)?))
}
