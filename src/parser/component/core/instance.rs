use std::collections::HashMap;
use crate::binary::BinaryReader;
use crate::component_model::{CoreInstance, CoreInstanceExport, CoreInstanceInlineExport, CoreInstantiate, CoreInstantiateArg, CoreSort, CoreSortType};
use crate::component_model::id::CoreFuncId;
use crate::parser::component::context::ParseContext;
use crate::parser::component::id::{parse_core_module_id, parse_instance_idx};
use crate::parser::component::ComponentParseError;
use crate::parser::component::core::parse_core_instance_idx;
use crate::parser::component::vec::VecParser;
use crate::parser::core::{parse_name, parse_u32, parse_vec};

type Result<R> = std::result::Result<R, ComponentParseError>;

pub fn parse_core_instance<R: BinaryReader>(
    ctx: &mut ParseContext<R>,
) -> Result<(usize, CoreInstance)> {
    let start = ctx.start_count();
    match ctx.reader.read_exact_one()? {
        0x00 => {
            let (_, idx) = parse_core_module_id(ctx)?;
            let mut imports = HashMap::new();
            for data in VecParser::new(ctx, parse_core_instantiate_arg)? {
                let (_, CoreInstantiateArg { name, instance_idx }) = data?;
                imports.insert(name, instance_idx);
            }
            Ok((
                ctx.end_count(start),
                CoreInstance {
                    module_idx: Some(idx),
                    exports: HashMap::new(),
                    imports,
                }
            ))
        }
        0x01 => {
            let mut exports = HashMap::new();
            for data in VecParser::new(ctx, parse_core_instance_inline_export)? {
                let (_, export) = data?;
                exports.insert(export.name.clone(), CoreInstanceExport::FuncReference(CoreFuncId(0)));
            }
            Ok((
                1,
                CoreInstance {
                    module_idx: None,
                    exports,
                    imports: Default::default(),
                },
            ))
        }
        magic => Err(ComponentParseError::InvalidInstanceExpr(magic)),
    }
}

fn parse_core_instantiate_arg<R: BinaryReader>(
    ctx: &mut ParseContext<R>,
) -> Result<(usize, CoreInstantiateArg)> {
    let (name_len, name) = parse_name(ctx.reader)?;
    ComponentParseError::assert_magic(
        [ctx.reader.read_exact_one()?],
        [0x12],
        "instantiate arg",
    )?;
    let (idx_len, instance_idx) = parse_core_instance_idx(ctx)?;
    Ok((
        name_len + 1 + idx_len,
        CoreInstantiateArg { name, instance_idx },
    ))
}

fn parse_core_instance_inline_export<R: BinaryReader>(
    ctx: &mut ParseContext<R>,
) -> Result<(usize, CoreInstanceInlineExport)> {
    let (name_len, name) = parse_name(ctx.reader)?;
    let (sort_len, sort) = parse_core_sort(ctx)?;
    // let (idx_len, sort_idx) = parse_sort_idx(ctx)?;
    // Ok((
    //     name_len + 1 + idx_len,
    //     CoreInstanceInlineExport {
    //         name,
    //         sort,
    //         sort_idx,
    //     },
    // ))
    Ok((name_len + sort_len, CoreInstanceInlineExport { name, sort }))
}

pub fn parse_core_sort<R: BinaryReader>(ctx: &mut ParseContext<R>) -> Result<(usize, CoreSort)> {
    let (len, ty) = parse_core_sort_type(ctx)?;
    let (idx_len, idx) = parse_u32(ctx.reader)?;
    let sort = match ty {
        CoreSortType::Func => CoreSort::Func(idx),
        CoreSortType::Table => CoreSort::Table(idx),
        CoreSortType::Memory => CoreSort::Memory(idx),
        CoreSortType::Global => CoreSort::Global(idx),
        CoreSortType::Type => CoreSort::Type(idx),
        CoreSortType::Module => CoreSort::Module(idx),
        CoreSortType::Instance => CoreSort::Instance(idx),
    };
    Ok((len + idx_len, sort))
}

pub(crate) fn parse_core_sort_type<R: BinaryReader>(
    ctx: &mut ParseContext<R>,
) -> Result<(usize, CoreSortType)> {
    let sort = match ctx.reader.read_exact_one()? {
        0x00 => CoreSortType::Func,
        0x01 => CoreSortType::Table,
        0x02 => CoreSortType::Memory,
        0x03 => CoreSortType::Global,
        0x10 => CoreSortType::Type,
        0x11 => CoreSortType::Module,
        0x12 => CoreSortType::Instance,
        magic => return Err(ComponentParseError::InvalidCoreSort(magic)),
    };
    Ok((1, sort))
}
