use crate::binary::BinaryReader;
use crate::component_model::id::{
    ComponentId, CoreSortId, FuncId, InstanceId, ModuleId, SortId, TypeId,
};
use crate::component_model::{CoreSort, SortType};
use crate::parser::component::parser::context::ParseContext;
use crate::parser::component::parser::ComponentModelParserError;
use crate::parser::core::{parse_i32, parse_u32};

type Result<R> = std::result::Result<R, ComponentModelParserError>;

pub fn parse_module_id<R: BinaryReader>(ctx: &mut ParseContext<R>) -> Result<(usize, ModuleId)> {
    let (len, id) = parse_u32(ctx.reader)?;
    // if let Some(id) = ctx.get_module_id(id) {
    //     Ok((len, id))
    // } else {
    //     Err(ComponentModelParserError::InvalidModuleId(id))
    // }
    todo!()
}

pub fn parse_instance_id<R: BinaryReader>(
    ctx: &mut ParseContext<R>,
) -> Result<(usize, InstanceId)> {
    let (len, id) = parse_u32(ctx.reader)?;
    // if let Some(id) = ctx.get_instance_id(id) {
    //     Ok((len, id))
    // } else {
    //     Err(ComponentModelParserError::InvalidInstanceId(id))
    // }
    todo!()
}

pub fn parse_component_id<R: BinaryReader>(
    ctx: &mut ParseContext<R>,
) -> Result<(usize, ComponentId)> {
    let (len, id) = parse_u32(ctx.reader)?;
    // if let Some(id) = ctx.get_component_id(id) {
    //     Ok((len, id))
    // } else {
    //     Err(ComponentModelParserError::InvalidComponentId(id))
    // }
    todo!()
}

pub fn parse_core_sort_id<R: BinaryReader>(
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

pub fn parse_sort_id<R: BinaryReader>(ctx: &mut ParseContext<R>) -> Result<(usize, SortId)> {
    todo!()
}

pub fn parse_type_id<R: BinaryReader>(ctx: &mut ParseContext<R>) -> Result<(usize, TypeId)> {
    let (len, id) = parse_i32(ctx.reader)?;
    assert!(id >= 0);
    Ok((len, TypeId(id)))
}

pub fn parse_func_id<R: BinaryReader>(ctx: &mut ParseContext<R>) -> Result<(usize, FuncId)> {
    todo!()
}
