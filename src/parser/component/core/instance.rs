use crate::binary::BinaryReader;
use crate::component_model::{
    CoreInstance, CoreInstanceInlineExport, CoreInstantiate, CoreInstantiateArg, CoreSort,
    CoreSortType,
};
use crate::parser::component::context::ParseContext;
use crate::parser::component::id::{parse_core_module_id, parse_instance_idx};
use crate::parser::component::ComponentModelParserError;
use crate::parser::core::{parse_name, parse_u32, parse_vec};

type Result<R> = std::result::Result<R, ComponentModelParserError>;

pub fn parse_core_instance<R: BinaryReader>(
    ctx: &mut ParseContext<R>,
) -> Result<(usize, CoreInstance)> {
    match ctx.reader.read_exact_one()? {
        0x00 => {
            let (idx_len, idx) = parse_core_module_id(ctx)?;
            let (args_len, args) = parse_vec(ctx, |v| v.reader, parse_core_instantiate_arg)?;
            Ok((
                1 + idx_len + args_len,
                CoreInstance::Instantiate(CoreInstantiate {
                    module_idx: idx,
                    args,
                }),
            ))
        }
        0x01 => {
            let (inline_exports_len, inline_exports) =
                parse_vec(ctx, |v| v.reader, parse_core_instance_inline_export)?;
            Ok((
                1 + inline_exports_len,
                CoreInstance::InlineExport(inline_exports),
            ))
        }
        magic => Err(ComponentModelParserError::InvalidInstanceExpr(magic)),
    }
}

fn parse_core_instantiate_arg<R: BinaryReader>(
    ctx: &mut ParseContext<R>,
) -> Result<(usize, CoreInstantiateArg)> {
    let (name_len, name) = parse_name(ctx.reader)?;
    ComponentModelParserError::assert_magic(
        [ctx.reader.read_exact_one()?],
        [0x12],
        "instantiate arg",
    )?;
    let (idx_len, instance_idx) = parse_instance_idx(ctx)?;
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
        magic => return Err(ComponentModelParserError::InvalidCoreSort(magic)),
    };
    Ok((1, sort))
}
