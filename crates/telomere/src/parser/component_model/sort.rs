use crate::binary::BinaryReader;
use crate::component_model::types::SortType;
use crate::component_model::Sort;
#[cfg(feature = "component-gated-feature-value-imports-exports")]
use crate::parser::component_model::parse_value_idx;
use crate::parser::component_model::{
    parse_component_local_idx, parse_func_local_idx, parse_instance_local_idx,
    parse_type_local_idx, ParseContext, ParseResult, SizedResult,
};

pub fn parse_sort(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<SortType> {
    let sort = match ctx.reader.read_exact_one()? {
        // 0x00 => Sort::Core(parse_core_sort(ctx)?.1),
        0x01 => SortType::Func,
        0x03 => SortType::Type,
        0x04 => SortType::Component,
        0x05 => SortType::Instance,
        _ => unreachable!(),
    };
    Ok(sort)
}

pub fn parse_sort_with_idx(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<Sort> {
    match ctx.reader.read_exact_one()? {
        0x01 => {
            let idx = parse_func_local_idx(ctx)?;
            let ty = ctx.validator.scope().func_indexes.get(idx)?;
            let idx = ctx.state.scope().funcs.get(idx)?;
            Ok(Sort::Func(idx, ty))
        }
        0x03 => {
            let idx = parse_type_local_idx(ctx)?;
            let id = ctx.validator.scope().type_indexes.get(idx)?;
            Ok(Sort::Type(id))
        }
        0x04 => {
            let idx = parse_component_local_idx(ctx)?;
            let ty = ctx.validator.scope().component_indexes.get(idx)?;
            let idx = ctx.state.scope().components.get(idx)?;
            Ok(Sort::Component(idx, ty))
        }
        0x05 => {
            let idx = parse_instance_local_idx(ctx)?;
            let ty = ctx.validator.scope().instance_indexes.get(idx)?;
            let idx = ctx.state.scope().instances.get(idx)?;
            Ok(Sort::Instance(idx, ty))
        }
        x => todo!("{}", x),
    }
}
