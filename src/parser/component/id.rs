use crate::binary::BinaryReader;
use crate::component_model::CoreModule;
use crate::component_model::id::{
    ComponentIdx, CoreModuleIdx, FuncId, InstanceIdx, SortId, TypeId,
};
use crate::parser::component::context::ParseContext;
use crate::parser::component::sort::TypeTable;
use crate::parser::component::ComponentParseError;
use crate::parser::core::{parse_i32, parse_u32};

type Result<R> = std::result::Result<R, ComponentParseError>;

pub fn parse_core_module_id<R: BinaryReader>(
    ctx: &mut ParseContext<R>,
) -> Result<(usize, CoreModuleIdx)> {
    let (len, id) = parse_u32(ctx.reader)?;
    if let Some(_) = ctx.builder.get_core_module(id as usize) {
        Ok((len, CoreModuleIdx(id as usize)))
    } else {
        Err(ComponentParseError::InvalidIdx(
            "Module".to_string(),
            id,
        ))
    }
}

pub fn parse_instance_idx<R: BinaryReader>(
    ctx: &mut ParseContext<R>,
) -> Result<(usize, InstanceIdx)> {
    let (len, id) = parse_u32(ctx.reader)?;
    // if let Some(idx) = ctx.sort.get_instance_idx(id as usize) {
    //     Ok((len, idx))
    // } else {
    //     Err(ComponentParseError::InvalidIdx(
    //         "Instance".to_string(),
    //         id,
    //     ))
    // }
    todo!()
}

pub fn parse_component_idx<R: BinaryReader>(
    ctx: &mut ParseContext<R>,
) -> Result<(usize, ComponentIdx)> {
    let (len, id) = parse_u32(ctx.reader)?;
    // if let Some(idx) = ctx.sort.get_component_idx(id as usize) {
    //     Ok((len, idx))
    // } else {
    //     Err(ComponentParseError::InvalidIdx(
    //         "Component".to_string(),
    //         id,
    //     ))
    // }
    todo!()
}

pub fn parse_sort_idx<R: BinaryReader>(ctx: &mut ParseContext<R>) -> Result<(usize, SortId)> {
    todo!()
}

pub fn parse_type_idx<R: BinaryReader>(ctx: &mut ParseContext<R>) -> Result<(usize, TypeId)> {
    let (len, id) = parse_i32(ctx.reader)?;
    assert!(id >= 0);
    Ok((len, TypeId(id)))
}

pub fn parse_func_idx<R: BinaryReader>(ctx: &mut ParseContext<R>) -> Result<(usize, FuncId)> {
    todo!()
}
