use crate::binary::BinaryReader;
use crate::component_model::id::{
    ComponentIdx, CoreModuleIdx, CoreSortId, FuncId, InstanceIdx, SortId, TypeId,
};
use crate::component_model::{CoreSort, SortType};
use crate::parser::component::parser::context::ParseContext;
use crate::parser::component::parser::ComponentModelParserError;
use crate::parser::core::{parse_i32, parse_u32};

type Result<R> = std::result::Result<R, ComponentModelParserError>;

pub fn parse_core_module_id<R: BinaryReader>(
    ctx: &mut ParseContext<R>,
) -> Result<(usize, CoreModuleIdx)> {
    let (len, id) = parse_u32(ctx.reader)?;
    if let Some(idx) = ctx.sort.get_core_module_idx(id as usize) {
        Ok((len, idx))
    } else {
        Err(ComponentModelParserError::InvalidIdx(
            "Module".to_string(),
            id,
        ))
    }
}

pub fn parse_instance_idx<R: BinaryReader>(
    ctx: &mut ParseContext<R>,
) -> Result<(usize, InstanceIdx)> {
    let (len, id) = parse_u32(ctx.reader)?;
    if let Some(idx) = ctx.sort.get_instance_idx(id as usize) {
        Ok((len, idx))
    } else {
        Err(ComponentModelParserError::InvalidIdx(
            "Instance".to_string(),
            id,
        ))
    }
}

pub fn parse_component_idx<R: BinaryReader>(
    ctx: &mut ParseContext<R>,
) -> Result<(usize, ComponentIdx)> {
    let (len, id) = parse_u32(ctx.reader)?;
    if let Some(idx) = ctx.sort.get_component_idx(id as usize) {
        Ok((len, idx))
    } else {
        Err(ComponentModelParserError::InvalidIdx(
            "Component".to_string(),
            id,
        ))
    }
}

pub fn parse_core_sort_idx<R: BinaryReader>(
    ctx: &mut ParseContext<R>,
    sort: &CoreSort,
) -> Result<(usize, CoreSortId)> {
    let (len, id) = parse_u32(ctx.reader)?;
    todo!()
    // match sort {
    //     CoreSort::Func => {todo!()}
    //     CoreSort::Table => {todo!()}
    //     CoreSort::Memory => {todo!()}
    //     CoreSort::Global => {todo!()}
    //     CoreSort::Type => {todo!()}
    //     CoreSort::Module => {todo!()}
    //     CoreSort::Instance => {todo!()}
    // }
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
