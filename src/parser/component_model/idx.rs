use crate::binary::BinaryReader;
#[cfg(feature = "component-gated-feature-value-imports-exports")]
use crate::component_model::ValueIdx;
use crate::component_model::{ComponentFunction, ComponentIdx, FuncIdx, InstanceIdx, Type, TypeIdx};
use crate::parser::component_model::context::ParseContext;
use crate::parser::component_model::{DefaultValidator, SizedResult, Validator};
use crate::parser::component_model::validator::IdxValidator;
use crate::parser::core::parse_u32;

pub fn parse_component_idx(
    ctx: &mut ParseContext<impl BinaryReader, impl DefaultValidator>,
) -> SizedResult<ComponentIdx> {
    let (len, idx) = parse_u32(ctx.reader)?;
    Ok((len, ctx.validator.validate_idx(idx)?))
}

pub fn parse_func_idx(
    ctx: &mut ParseContext<impl BinaryReader, impl DefaultValidator>,
) -> SizedResult<FuncIdx> {
    let (len, idx) = parse_u32(ctx.reader)?;
    Ok((len, ctx.validator.validate_idx(idx)?))
}

pub fn parse_func_idx_resolved(
    ctx: &mut ParseContext<impl BinaryReader, impl Validator + IdxValidator<FuncIdx, ComponentFunction>>,
) -> SizedResult<ComponentFunction> {
    let (len, idx) = parse_u32(ctx.reader)?;
    Ok((len, ctx.validator.validate_idx_resolved(idx)?))
}

pub fn parse_type_idx(
    ctx: &mut ParseContext<impl BinaryReader, impl DefaultValidator>,
) -> SizedResult<TypeIdx> {
    let (len, idx) = parse_u32(ctx.reader)?;
    Ok((len, ctx.validator.validate_idx(idx)?))
}

pub fn parse_type_idx_resolved(
    ctx: &mut ParseContext<impl BinaryReader, impl Validator + IdxValidator<TypeIdx, Type>>,
) -> SizedResult<Type> {
    let (len, idx) = parse_u32(ctx.reader)?;
    Ok((len, ctx.validator.validate_idx_resolved(idx)?))
}

pub fn parse_instance_idx(
    ctx: &mut ParseContext<impl BinaryReader, impl DefaultValidator>,
) -> SizedResult<InstanceIdx> {
    let (len, idx) = parse_u32(ctx.reader)?;
    Ok((len, ctx.validator.validate_idx(idx)?))
}

#[cfg(feature = "component-gated-feature-value-imports-exports")]
pub fn parse_value_idx(ctx: &mut ParseContext<impl BinaryReader>) -> SizedResult<ValueIdx> {
    let (len, idx) = parse_u32(ctx.reader)?;
    Ok((len, ctx.validator.validate_value_idx(idx as usize)?))
}
