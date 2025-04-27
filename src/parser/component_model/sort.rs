use crate::binary::BinaryReader;
use crate::component_model::{CoreSort, CoreSortWithIdx, Sort, SortWithIdx};
#[cfg(feature = "component-gated-feature-value-imports-exports")]
use crate::parser::component_model::parse_value_idx;
use crate::parser::component_model::{parse_component_idx, parse_core_func_idx, parse_core_global_idx, parse_core_instance_idx, parse_core_memory_idx, parse_core_module_idx, parse_core_sort, parse_core_table_idx, parse_core_type_idx, parse_func_idx, parse_instance_idx, parse_type_idx, DefaultValidator, ParseContext, SizedResult, Validator};

pub fn parse_sort(ctx: &mut ParseContext<impl BinaryReader, impl DefaultValidator>) -> SizedResult<Sort> {
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

pub fn parse_sort_with_idx(
    ctx: &mut ParseContext<impl BinaryReader, impl DefaultValidator>,
) -> SizedResult<SortWithIdx> {
    let start_count = ctx.reader.read_count();

    match ctx.reader.read_exact_one()? {
        0x00 => {
            let (_, sort) = parse_core_sort(ctx)?;
            match sort {
                CoreSort::Func => {
                    let (_, func_idx) = parse_core_func_idx(ctx)?;
                    Ok((
                        ctx.reader.read_count() - start_count,
                        SortWithIdx::Core(CoreSortWithIdx::Func(func_idx)),
                    ))
                }
                CoreSort::Table => {
                    let (_, table_idx) = parse_core_table_idx(ctx)?;
                    Ok((
                        ctx.reader.read_count() - start_count,
                        SortWithIdx::Core(CoreSortWithIdx::Table(table_idx)),
                    ))
                }
                CoreSort::Memory => {
                    let (_, memory_idx) = parse_core_memory_idx(ctx)?;
                    Ok((
                        ctx.reader.read_count() - start_count,
                        SortWithIdx::Core(CoreSortWithIdx::Memory(memory_idx)),
                    ))
                }
                CoreSort::Global => {
                    let (_, global_idx) = parse_core_global_idx(ctx)?;
                    Ok((
                        ctx.reader.read_count() - start_count,
                        SortWithIdx::Core(CoreSortWithIdx::Global(global_idx)),
                    ))
                }
                CoreSort::Type => {
                    let (_, type_idx) = parse_core_type_idx(ctx)?;
                    Ok((
                        ctx.reader.read_count() - start_count,
                        SortWithIdx::Core(CoreSortWithIdx::Type(type_idx)),
                    ))
                }
                CoreSort::Module => {
                    let (_, module_idx) = parse_core_module_idx(ctx)?;
                    Ok((
                        ctx.reader.read_count() - start_count,
                        SortWithIdx::Core(CoreSortWithIdx::Module(module_idx)),
                    ))
                }
                CoreSort::Instance => {
                    let (_, instance_idx) = parse_core_instance_idx(ctx)?;
                    Ok((
                        ctx.reader.read_count() - start_count,
                        SortWithIdx::Core(CoreSortWithIdx::Instance(instance_idx)),
                    ))
                }
            }
        }
        0x01 => {
            let (_, idx) = parse_func_idx(ctx)?;
            Ok((
                ctx.reader.read_count() - start_count,
                SortWithIdx::Func(idx),
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
            let (_, idx) = parse_type_idx(ctx)?;
            Ok((
                ctx.reader.read_count() - start_count,
                SortWithIdx::Type(idx),
            ))
        }
        0x04 => {
            let (_, idx) = parse_component_idx(ctx)?;
            Ok((
                ctx.reader.read_count() - start_count,
                SortWithIdx::Component(idx),
            ))
        }
        0x05 => {
            let (_, idx) = parse_instance_idx(ctx)?;
            Ok((
                ctx.reader.read_count() - start_count,
                SortWithIdx::Instance(idx),
            ))
        }
        _ => unreachable!(),
    }
}
