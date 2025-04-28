use crate::binary::BinaryReader;
use crate::component_model::CoreSort;
use crate::parser::component_model::context::ParseContext;
use crate::parser::component_model::error::ComponentParseError;
use crate::parser::component_model::{SizedResult, Validator};

pub fn parse_core_sort(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> SizedResult<CoreSort> {
    let sort = match ctx.reader.read_exact_one()? {
        0x00 => CoreSort::Func,
        0x01 => CoreSort::Table,
        0x02 => CoreSort::Memory,
        0x03 => CoreSort::Global,
        0x10 => CoreSort::Type,
        0x11 => CoreSort::Module,
        0x12 => CoreSort::Instance,
        magic => return Err(ComponentParseError::InvalidCoreSort(magic)),
    };
    Ok((1, sort))
}
