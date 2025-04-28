use crate::binary::BinaryReader;
use crate::component_model::{
    CoreFuncIdx, CoreGlobalIdx, CoreInstanceIdx, CoreMemoryIdx, CoreModuleIdx, CoreTableIdx,
    CoreType, CoreTypeIdx, Resolver,
};
use crate::parser::component_model::context::ParseContext;
use crate::parser::component_model::validator::{
    DefaultValidatorState, IdxValidator, ValidatorStateImpl,
};
use crate::parser::component_model::{ComponentParseError, ParseResult, Validator};
use crate::parser::core::parse_u32;

pub fn parse_core_module_idx(
    ctx: &mut ParseContext<impl BinaryReader, impl DefaultValidatorState>,
) -> ParseResult<CoreModuleIdx> {
    let (len, idx) = parse_u32(ctx.reader)?;
    Ok(ctx.validator.validate_local_idx(idx)?)
}

pub fn parse_core_instance_idx(
    ctx: &mut ParseContext<impl BinaryReader, impl DefaultValidatorState>,
) -> ParseResult<CoreInstanceIdx> {
    let (len, idx) = parse_u32(ctx.reader)?;
    Ok(ctx.validator.validate_local_idx(idx)?)
}

pub fn parse_core_func_idx(
    ctx: &mut ParseContext<impl BinaryReader, impl DefaultValidatorState>,
) -> ParseResult<CoreFuncIdx> {
    let (len, idx) = parse_u32(ctx.reader)?;
    Ok(ctx.validator.validate_local_idx(idx)?)
}

pub fn parse_core_memory_idx(
    ctx: &mut ParseContext<impl BinaryReader, impl DefaultValidatorState>,
) -> ParseResult<CoreMemoryIdx> {
    let (len, idx) = parse_u32(ctx.reader)?;
    Ok(ctx.validator.validate_local_idx(idx)?)
}

pub fn parse_core_type_idx(
    ctx: &mut ParseContext<impl BinaryReader, impl DefaultValidatorState>,
) -> ParseResult<CoreTypeIdx> {
    let (len, idx) = parse_u32(ctx.reader)?;
    Ok(ctx.validator.validate_local_idx(idx)?)
}

pub fn parse_core_type_idx_resolved(
    ctx: &mut ParseContext<
        impl BinaryReader,
        impl ValidatorStateImpl
            + IdxValidator<CoreTypeIdx, Resolved = CoreType>
            + Resolver<CoreType, Error = ComponentParseError>,
    >,
) -> ParseResult<CoreType> {
    let (len, idx) = parse_u32(ctx.reader)?;
    Ok(ctx.validator.validate_idx_resolved(idx)?)
}

pub fn parse_core_table_idx(
    ctx: &mut ParseContext<impl BinaryReader, impl DefaultValidatorState>,
) -> ParseResult<CoreTableIdx> {
    let (len, idx) = parse_u32(ctx.reader)?;
    Ok(ctx.validator.validate_local_idx(idx)?)
}

pub fn parse_core_global_idx(
    ctx: &mut ParseContext<impl BinaryReader, impl DefaultValidatorState>,
) -> ParseResult<CoreGlobalIdx> {
    let (len, idx) = parse_u32(ctx.reader)?;
    Ok(ctx.validator.validate_local_idx(idx)?)
}
