use crate::decoder::context::ParseContext;
use crate::decoder::error::ComponentParseError;
use crate::decoder::ParseResult;
use crate::ir::types::CoreSortType;
use crate::support::binary::BinaryReader;

pub fn parse_core_sort(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<CoreSortType> {
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
    Ok(sort)
}
