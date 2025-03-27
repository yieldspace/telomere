use crate::assert_magic;
use crate::binary::BinaryReader;
use crate::component::{
    CoreInstance, CoreInstanceInlineExport, CoreInstantiate, CoreInstantiateArg, CoreSort,
    InlineExport, Instance, Instantiate, InstantiateArg, Sort,
};
use crate::parser::component::parser::ComponentModelParserError;
use crate::parser::core::{parse_name, parse_u32, parse_vec};

type Result<R> = std::result::Result<R, ComponentModelParserError>;

pub fn parse_instance<R: BinaryReader>(reader: &mut R) -> Result<(usize, Instance)> {
    match reader.read_exact_one()? {
        0x00 => {
            let (_, component_idx) = parse_u32(reader)?;
            let (args_len, args) = parse_vec(reader, |v| v, parse_instantiate_arg)?;
            Ok((
                1 + component_idx as usize + args_len,
                Instance::Instantiate(Instantiate {
                    component_idx: component_idx as usize,
                    args,
                }),
            ))
        }
        0x01 => {
            let (exports_len, exports) = parse_vec(reader, |v| v, parse_inline_export)?;
            Ok((1 + exports_len, Instance::InlineExport(exports)))
        }
        _ => todo!(),
    }
}

fn parse_instantiate_arg<R: BinaryReader>(reader: &mut R) -> Result<(usize, InstantiateArg)> {
    let (name_len, name) = parse_name(reader)?;
    let (len, sort) = parse_sort(reader)?;
    let (idx_len, sort_idx) = parse_u32(reader)?;
    Ok((
        name_len + len + idx_len,
        InstantiateArg {
            name,
            sort,
            sort_idx: sort_idx as usize,
        },
    ))
}

pub(crate) fn parse_sort<R: BinaryReader>(reader: &mut R) -> Result<(usize, Sort)> {
    Ok(match reader.read_exact_one()? {
        0x00 => {
            let (cs_len, core_sort) = parse_core_sort(reader)?;
            (1 + cs_len, Sort::Core(core_sort))
        }
        0x01 => (1, Sort::Func),
        0x02 => (1, Sort::Value),
        0x03 => (1, Sort::Type),
        0x04 => (1, Sort::Component),
        0x05 => (1, Sort::Instance),
        sort => return Err(ComponentModelParserError::InvalidSort(sort)),
    })
}

fn parse_inline_export<R: BinaryReader>(reader: &mut R) -> Result<(usize, InlineExport)> {
    let (name_len, name) = parse_name(reader)?;
    let (len, sort) = parse_sort(reader)?;
    let (idx_len, sort_idx) = parse_u32(reader)?;
    Ok((
        name_len + len + idx_len,
        InlineExport {
            name,
            sort,
            sort_idx: sort_idx as usize,
        },
    ))
}

pub fn parse_core_instance<R: BinaryReader>(reader: &mut R) -> Result<(usize, CoreInstance)> {
    match reader.read_exact_one()? {
        0x00 => {
            let (idx_len, idx) = parse_u32(reader)?;
            let (args_len, args) = parse_vec(reader, |v| v, parse_core_instantiate_arg)?;
            Ok((
                1 + idx_len + args_len,
                CoreInstance::Instantiate(CoreInstantiate {
                    module_idx: idx as usize,
                    args,
                }),
            ))
        }
        0x01 => {
            let (inline_exports_len, inline_exports) =
                parse_vec(reader, |v| v, parse_core_instance_inline_export)?;
            Ok((
                1 + inline_exports_len,
                CoreInstance::InlineExport(inline_exports),
            ))
        }
        magic => Err(ComponentModelParserError::InvalidInstanceExpr(magic)),
    }
}

fn parse_core_instantiate_arg<R: BinaryReader>(
    reader: &mut R,
) -> Result<(usize, CoreInstantiateArg)> {
    let (name_len, name) = parse_name(reader)?;
    assert_magic!(
        reader.read_exact_one()?,
        0x12,
        ComponentModelParserError::InvalidInstantiateArgMagic
    );
    let (idx_len, instance_idx) = parse_u32(reader)?;
    Ok((
        name_len + 1 + idx_len,
        CoreInstantiateArg {
            name,
            instance_idx: instance_idx as usize,
        },
    ))
}

fn parse_core_instance_inline_export<R: BinaryReader>(
    reader: &mut R,
) -> Result<(usize, CoreInstanceInlineExport)> {
    let (name_len, name) = parse_name(reader)?;
    let (_, sort) = parse_core_sort(reader)?;
    let (idx_len, sort_idx) = parse_u32(reader)?;
    Ok((
        name_len + 1 + idx_len,
        CoreInstanceInlineExport {
            name,
            sort,
            sort_idx: sort_idx as usize,
        },
    ))
}

pub(crate) fn parse_core_sort<R: BinaryReader>(reader: &mut R) -> Result<(usize, CoreSort)> {
    let sort = match reader.read_exact_one()? {
        0x00 => CoreSort::Func,
        0x01 => CoreSort::Table,
        0x02 => CoreSort::Memory,
        0x03 => CoreSort::Global,
        0x10 => CoreSort::Type,
        0x11 => CoreSort::Module,
        0x12 => CoreSort::Instance,
        magic => return Err(ComponentModelParserError::InvalidCoreSort(magic)),
    };
    Ok((1, sort))
}
