use crate::decoder::{ParseContext, ParseResult};
use crate::ir::types::Type;
use crate::ir::{Component, Func, Instance, LocalIdx};
use crate::support::binary::BinaryReader;
use crate::support::parser::core::parse_u32;

pub fn parse_func_local_idx(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> ParseResult<LocalIdx<Func>> {
    let (_, idx) = parse_u32(ctx.reader)?;
    Ok(LocalIdx::new(idx))
}

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
