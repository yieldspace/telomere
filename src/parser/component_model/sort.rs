use crate::binary::BinaryReader;
use crate::component_model::{CoreSort, CoreSortWithIdx, Sort, SortWithIdx};
#[cfg(feature = "component-gated-feature-value-imports-exports")]
use crate::parser::component_model::parse_value_idx;
use crate::parser::component_model::{
    parse_component_idx, parse_core_func_idx, parse_core_global_idx, parse_core_instance_idx,
    parse_core_memory_idx, parse_core_module_idx, parse_core_sort, parse_core_table_idx,
    parse_core_type_idx, parse_func_idx, parse_instance_idx, parse_type_idx, ParseContext,
    SizedResult,
};

pub fn parse_sort(ctx: &mut ParseContext<impl BinaryReader>) -> SizedResult<Sort> {
    let start_count = ctx.reader.read_count();

    let sort = match ctx.reader.read_exact_one()? {
        0x00 => Sort::Core(parse_core_sort(ctx)?.1),
        0x01 => Sort::Func,
        #[cfg(feature = "component-gated-feature-value-imports-exports")]
        0x02 => Sort::Value,
        0x03 => Sort::Type,
        0x04 => Sort::Component,
        0x05 => Sort::Instance,
        _ => unreachable!(),
    };
    Ok((ctx.reader.read_count() - start_count, sort))
}

pub fn parse_sort_with_idx(ctx: &mut ParseContext<impl BinaryReader>) -> SizedResult<SortWithIdx> {
    let start_count = ctx.reader.read_count();

    match ctx.reader.read_exact_one()? {
        0x00 => {
            let (_, sort) = parse_core_sort(ctx)?;
            match sort {
                CoreSort::Func => {
                    let idx = parse_core_func_idx(ctx)?;
                    let ty = ctx.validator.get_core_func_type(idx)?;
                    let func_idx = ctx.validator.get_global_core_func(idx)?;
                    Ok((
                        ctx.reader.read_count() - start_count,
                        SortWithIdx::Core(CoreSortWithIdx::Func(func_idx, ty)),
                    ))
                }
                CoreSort::Table => {
                    let idx = parse_core_table_idx(ctx)?;
                    let ty = ctx.validator.get_core_table_type(idx)?;
                    let table_idx = ctx.validator.get_global_core_table(idx)?;
                    Ok((
                        ctx.reader.read_count() - start_count,
                        SortWithIdx::Core(CoreSortWithIdx::Table(table_idx, ty)),
                    ))
                }
                CoreSort::Memory => {
                    let idx = parse_core_memory_idx(ctx)?;
                    let ty = ctx.validator.get_core_memory_type(idx)?;
                    let memory_idx = ctx.validator.get_global_core_memory(idx)?;
                    Ok((
                        ctx.reader.read_count() - start_count,
                        SortWithIdx::Core(CoreSortWithIdx::Memory(memory_idx, ty)),
                    ))
                }
                CoreSort::Global => {
                    let idx = parse_core_global_idx(ctx)?;
                    let ty = ctx.validator.get_core_global_type(idx)?;
                    let global_idx = ctx.validator.get_global_core_global(idx)?;
                    Ok((
                        ctx.reader.read_count() - start_count,
                        SortWithIdx::Core(CoreSortWithIdx::Global(global_idx, ty)),
                    ))
                }
                CoreSort::Type => {
                    let idx = parse_core_type_idx(ctx)?;
                    let ty = ctx.validator.get_core_type(idx)?;
                    Ok((
                        ctx.reader.read_count() - start_count,
                        SortWithIdx::Core(CoreSortWithIdx::Type(ty)),
                    ))
                }
                CoreSort::Module => {
                    let idx = parse_core_module_idx(ctx)?;
                    let ty = ctx.validator.get_core_module_type(idx)?;
                    let module_idx = ctx.validator.get_global_core_module(idx)?;
                    Ok((
                        ctx.reader.read_count() - start_count,
                        SortWithIdx::Core(CoreSortWithIdx::Module(module_idx, ty)),
                    ))
                }
                CoreSort::Instance => {
                    let idx = parse_core_instance_idx(ctx)?;
                    let ty = ctx.validator.get_core_instance_type(idx)?;
                    let instance_idx = ctx.validator.get_global_core_instance(idx)?;
                    Ok((
                        ctx.reader.read_count() - start_count,
                        SortWithIdx::Core(CoreSortWithIdx::Instance(instance_idx, ty)),
                    ))
                }
            }
        }
        0x01 => {
            let idx = parse_func_idx(ctx)?;
            let ty = ctx.validator.get_func_type(idx)?;
            let idx = ctx.validator.get_global_func(idx)?;
            Ok((
                ctx.reader.read_count() - start_count,
                SortWithIdx::Func(idx, ty),
            ))
        }
        #[cfg(feature = "component-gated-feature-value-imports-exports")]
        0x02 => {
            let (_, idx) = parse_value_idx(ctx)?;
            Ok((
                ctx.reader.read_count() - start_count,
                SortWithIdx::Value(idx),
            ))
        }
        0x03 => {
            let idx = parse_type_idx(ctx)?;
            let ty = ctx.validator.get_type(idx)?;
            Ok((ctx.reader.read_count() - start_count, SortWithIdx::Type(ty)))
        }
        0x04 => {
            let idx = parse_component_idx(ctx)?;
            let ty = ctx.validator.get_component_type(idx)?;
            let idx = ctx.validator.get_global_component(idx)?;
            Ok((
                ctx.reader.read_count() - start_count,
                SortWithIdx::Component(idx, ty),
            ))
        }
        0x05 => {
            let idx = parse_instance_idx(ctx)?;
            let ty = ctx.validator.get_instance_type(idx)?;
            let idx = ctx.validator.get_global_instance(idx)?;
            Ok((
                ctx.reader.read_count() - start_count,
                SortWithIdx::Instance(idx, ty),
            ))
        }
        _ => unreachable!(),
    }
}
