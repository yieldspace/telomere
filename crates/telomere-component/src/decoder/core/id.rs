use crate::decoder::context::ParseContext;
use crate::decoder::ParseResult;
use crate::ir::types::CoreType;
use crate::ir::{CoreFunc, CoreInstance, CoreMemory, CoreModule, LocalIdx};
use crate::support::binary::BinaryReader;
use crate::support::parser::core::parse_u32;

pub fn parse_core_module_local_idx(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> ParseResult<LocalIdx<CoreModule>> {
    let (_, idx) = parse_u32(ctx.reader)?;
    Ok(LocalIdx::new(idx))
}

pub fn parse_core_instance_local_idx(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> ParseResult<LocalIdx<CoreInstance>> {
    let (_, idx) = parse_u32(ctx.reader)?;
    Ok(LocalIdx::new(idx))
}

pub fn parse_core_type_local_idx(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> ParseResult<LocalIdx<CoreType>> {
    let (_, idx) = parse_u32(ctx.reader)?;
    Ok(LocalIdx::new(idx))
}

pub fn parse_core_func_local_idx(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> ParseResult<LocalIdx<CoreFunc>> {
    let (_, idx) = parse_u32(ctx.reader)?;
    Ok(LocalIdx::new(idx))
}

pub fn parse_core_memory_local_idx(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> ParseResult<LocalIdx<CoreMemory>> {
    let (_, idx) = parse_u32(ctx.reader)?;
    Ok(LocalIdx::new(idx))
}

// pub fn parse_core_func_idx(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<LocalIdx> {
//     let (_, idx) = parse_u32(ctx.reader)?;
//     ctx.validator.validate_core_func_idx(idx)
// }

// pub fn parse_core_memory_idx(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<LocalIdx> {
//     let (_, idx) = parse_u32(ctx.reader)?;
//     ctx.validator.validate_core_memory_idx(idx)
// }

// pub fn parse_core_type_idx(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<LocalIdx<CoreType>> {
//     let (_, idx) = parse_u32(ctx.reader)?;
//     ctx.validator.validate_core_type_idx(idx)
// }

// pub fn parse_core_table_idx(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<LocalIdx> {
//     let (_, idx) = parse_u32(ctx.reader)?;
//     ctx.validator.validate_core_table_idx(idx)
// }

// pub fn parse_core_global_idx(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<LocalIdx> {
//     let (_, idx) = parse_u32(ctx.reader)?;
//     ctx.validator.validate_core_global_idx(idx)
// }
