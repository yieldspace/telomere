use crate::binary::BinaryReader;
use crate::component_model::types::Type;
use crate::component_model::{Component, Instance, LocalIdx};
use crate::parser::component_model::{ParseContext, ParseResult};
use crate::parser::core::parse_u32;

pub fn parse_component_local_idx(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> ParseResult<LocalIdx<Component>> {
    let (_, idx) = parse_u32(ctx.reader)?;
    Ok(LocalIdx::new(idx))
}

pub fn parse_instance_local_idx(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> ParseResult<LocalIdx<Instance>> {
    let (_, idx) = parse_u32(ctx.reader)?;
    Ok(LocalIdx::new(idx))
}

pub fn parse_type_local_idx(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> ParseResult<LocalIdx<Type>> {
    let (_, idx) = parse_u32(ctx.reader)?;
    Ok(LocalIdx::new(idx))
}
