use crate::binary::BinaryReader;
#[cfg(feature = "component-gated-feature-value-imports-exports")]
use crate::component_model::ValueIdx;
use crate::component_model::{
    ComponentFunction, ComponentIdx, FuncIdx, InlineComponent, Instance, InstanceIdx, Resolver,
    Type, TypeIdx,
};
use crate::parser::component_model::context::ParseContext;
use crate::parser::component_model::validator::{
    DefaultValidatorState, IdxValidator, ValidatorStateImpl,
};
use crate::parser::component_model::{ComponentParseError, ParseResult, Validator};
use crate::parser::core::parse_u32;

pub fn parse_component_idx(
    ctx: &mut ParseContext<impl BinaryReader, impl ValidatorStateImpl + IdxValidator<ComponentIdx>>,
) -> ParseResult<ComponentIdx> {
    let (_, idx) = parse_u32(ctx.reader)?;
    Ok(ctx.validator.validate_local_idx(idx)?)
}

pub fn parse_func_idx(
    ctx: &mut ParseContext<impl BinaryReader, impl ValidatorStateImpl + IdxValidator<FuncIdx>>,
) -> ParseResult<FuncIdx> {
    let (_, idx) = parse_u32(ctx.reader)?;
    Ok(ctx.validator.validate_local_idx(idx)?)
}

pub fn parse_func_idx_resolved(
    ctx: &mut ParseContext<
        impl BinaryReader,
        impl ValidatorStateImpl
            + IdxValidator<FuncIdx, Resolved = ComponentFunction>
            + Resolver<ComponentFunction, Error = ComponentParseError>,
    >,
) -> ParseResult<ComponentFunction> {
    let (_, idx) = parse_u32(ctx.reader)?;
    Ok(ctx.validator.state.validate_idx_resolved(idx)?)
}

pub fn parse_type_idx(
    ctx: &mut ParseContext<impl BinaryReader, impl ValidatorStateImpl + IdxValidator<TypeIdx>>,
) -> ParseResult<TypeIdx> {
    let (_, idx) = parse_u32(ctx.reader)?;
    Ok(ctx.validator.validate_local_idx(idx)?)
}

pub fn parse_type_idx_resolved(
    ctx: &mut ParseContext<
        impl BinaryReader,
        impl ValidatorStateImpl
            + IdxValidator<TypeIdx, Resolved = Type>
            + Resolver<Type, Error = ComponentParseError>,
    >,
) -> ParseResult<Type> {
    let (_, idx) = parse_u32(ctx.reader)?;
    Ok(ctx.validator.validate_idx_resolved(idx)?)
}

pub fn parse_instance_idx(
    ctx: &mut ParseContext<impl BinaryReader, impl ValidatorStateImpl + IdxValidator<InstanceIdx>>,
) -> ParseResult<InstanceIdx> {
    let (_, idx) = parse_u32(ctx.reader)?;
    Ok(ctx.validator.validate_local_idx(idx)?)
}

pub fn parse_instance_idx_resolved(
    ctx: &mut ParseContext<impl BinaryReader, impl ValidatorStateImpl + IdxValidator<InstanceIdx, Resolved=Instance> + Resolver<Instance, Error = ComponentParseError>>,
) -> ParseResult<Instance> {
    let (_, idx) = parse_u32(ctx.reader)?;
    Ok(ctx.validator.validate_idx_resolved(idx)?)
}

#[cfg(feature = "component-gated-feature-value-imports-exports")]
pub fn parse_value_idx(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<ValueIdx> {
    let (len, idx) = parse_u32(ctx.reader)?;
    Ok(ctx.validator.validate_value_idx(idx as usize)?)
}
