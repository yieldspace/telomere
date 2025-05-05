use crate::binary::BinaryReader;
#[cfg(feature = "component-gated-feature-value-imports-exports")]
use crate::component_model::ValueIdx;
use crate::parser::component_model::context::ParseContext;
use crate::parser::component_model::validator::LocalIdx;
use crate::parser::component_model::ParseResult;
use crate::parser::core::parse_u32;

pub fn parse_component_idx(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<LocalIdx> {
    let (_, idx) = parse_u32(ctx.reader)?;
    ctx.validator.validate_component_idx(idx)
}

pub fn parse_func_idx(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<LocalIdx> {
    let (_, idx) = parse_u32(ctx.reader)?;
    ctx.validator.validate_func_idx(idx)
}

pub fn parse_type_idx(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<LocalIdx> {
    let (_, idx) = parse_u32(ctx.reader)?;
    ctx.validator.validate_type_idx(idx)
}

pub fn parse_instance_idx(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<LocalIdx> {
    let (_, idx) = parse_u32(ctx.reader)?;
    ctx.validator.validate_instance_idx(idx)
}

#[cfg(feature = "component-gated-feature-value-imports-exports")]
pub fn parse_value_idx(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<ValueIdx> {
    let (len, idx) = parse_u32(ctx.reader)?;
    Ok(ctx.validator.validate_value_idx(idx as usize)?)
}
