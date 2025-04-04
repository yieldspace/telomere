use crate::assert_magic;
use crate::binary::{BinaryReader, Countable, Counter};
use crate::component_model::id::ComponentIdx;
use crate::component_model::{
    CoreInstance, CoreInstanceInlineExport, CoreInstantiate, CoreInstantiateArg, CoreSort,
    InlineExport, Instance, Instantiate, InstantiateArg, Sort, SortType,
};
use crate::parser::component::parser::context::ParseContext;
use crate::parser::component::parser::core::parse_core_sort;
use crate::parser::component::parser::id::parse_component_idx;
use crate::parser::component::parser::ComponentModelParserError;
use crate::parser::core::{parse_name, parse_u32, parse_vec};

type Result<R> = std::result::Result<R, ComponentModelParserError>;

pub fn parse_instance<R: BinaryReader>(ctx: &mut ParseContext<R>) -> Result<(usize, Instance)> {
    let mut counter = Counter::new();
    match ctx.reader.read_exact_one()?.count(&mut counter) {
        0x00 => {
            let component_idx = parse_component_idx(ctx)?.count(&mut counter);
            let args = parse_vec(ctx, |v| v.reader, parse_instantiate_arg)?.count(&mut counter);
            Ok((
                counter.count(),
                Instance::Instantiate(Instantiate {
                    component_idx,
                    args,
                }),
            ))
        }
        0x01 => {
            let exports = parse_vec(ctx, |v| v.reader, parse_inline_export)?.count(&mut counter);
            Ok((counter.count(), Instance::InlineExport(exports)))
        }
        _ => todo!(),
    }
}

fn parse_instantiate_arg<R: BinaryReader>(
    ctx: &mut ParseContext<R>,
) -> Result<(usize, InstantiateArg)> {
    let (name_len, name) = parse_name(ctx.reader)?;
    let (len, sort) = parse_sort(ctx)?;
    Ok((name_len + len, InstantiateArg { name, sort }))
}

pub(crate) fn parse_sort<R: BinaryReader>(ctx: &mut ParseContext<R>) -> Result<(usize, Sort)> {
    let (type_len, sort_type) = parse_sort_type(ctx)?;
    let (idx_len, idx) = parse_u32(ctx.reader)?;
    let sort = match sort_type {
        SortType::Core(coresort) => Sort::Core(coresort, idx as usize),
        SortType::Func => {
            todo!()
        }
        SortType::Value => {
            todo!()
        }
        SortType::Type => {
            todo!()
        }
        SortType::Component => {
            // match ctx.get_component_id(idx) {
            //     None => return Err(ComponentModelParserError::InvalidComponentId(idx)),
            //     Some(id) => {
            //         Sort::Component(id, idx as usize)
            //     }
            // }
            todo!()
        }
        SortType::Instance => {
            // match ctx.get_instance_id(idx) {
            //     None => return Err(ComponentModelParserError::InvalidInstanceId(idx)),
            //     Some(id) => {
            //         Sort::Instance(id, idx as usize)
            //     }
            // }
            todo!()
        }
    };
    Ok((type_len + idx_len, sort))
}

pub(crate) fn parse_sort_type<R: BinaryReader>(
    ctx: &mut ParseContext<R>,
) -> Result<(usize, SortType)> {
    Ok(match ctx.reader.read_exact_one()? {
        0x00 => {
            let (cs_len, core_sort) = parse_core_sort(ctx)?;
            (1 + cs_len, SortType::Core(core_sort))
        }
        0x01 => (1, SortType::Func),
        0x02 => (1, SortType::Value),
        0x03 => (1, SortType::Type),
        0x04 => (1, SortType::Component),
        0x05 => (1, SortType::Instance),
        sort => return Err(ComponentModelParserError::InvalidSort(sort)),
    })
}

fn parse_inline_export<R: BinaryReader>(
    ctx: &mut ParseContext<R>,
) -> Result<(usize, InlineExport)> {
    let (name_len, name) = parse_name(ctx.reader)?;
    let (len, sort) = parse_sort(ctx)?;
    Ok((name_len + len, InlineExport { name, sort }))
}
