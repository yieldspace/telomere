use crate::binary::BinaryReader;
use crate::component_model::{CoreInstanceIdx, CoreModuleIdx};
use crate::parser::component_model::context::ParseContext;
use crate::parser::component_model::error::ComponentParseError;
use crate::parser::component_model::validator::Validator;
use crate::parser::component_model::SizedResult;
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
