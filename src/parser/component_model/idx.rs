use crate::binary::BinaryReader;
use crate::component_model::{ComponentIdx, FuncIdx, InstanceIdx, TypeIdx};
use crate::parser::component_model::context::ParseContext;
use crate::parser::component_model::validator::Validator;
use crate::parser::component_model::SizedResult;
use crate::parser::core::parse_u32;

pub fn parse_component_idx(
    ctx: &mut ParseContext<impl BinaryReader, impl Validator>,
) -> SizedResult<ComponentIdx> {
    let (len, idx) = parse_u32(ctx.reader)?;
    Ok((len, ctx.validator.validate_component_idx(idx as usize)?))
}

pub fn parse_func_idx(
    ctx: &mut ParseContext<impl BinaryReader, impl Validator>,
) -> SizedResult<FuncIdx> {
    let (len, idx) = parse_u32(ctx.reader)?;
    Ok((len, ctx.validator.validate_function_idx(idx as usize)?))
}

pub fn parse_type_idx(
    ctx: &mut ParseContext<impl BinaryReader, impl Validator>,
) -> SizedResult<TypeIdx> {
    let (len, idx) = parse_u32(ctx.reader)?;
    Ok((len, ctx.validator.validate_type_idx(idx as usize)?))
}

pub fn parse_instance_idx(
    ctx: &mut ParseContext<impl BinaryReader, impl Validator>,
) -> SizedResult<InstanceIdx> {
    let (len, idx) = parse_u32(ctx.reader)?;
    Ok((len, ctx.validator.validate_instance_idx(idx as usize)?))
}
