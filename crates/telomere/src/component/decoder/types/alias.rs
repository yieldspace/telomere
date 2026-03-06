use crate::binary::BinaryReader;
use crate::component::decoder::sort::parse_sort;
use crate::component::decoder::{
    parse_instance_local_idx, ComponentParseError, ParseContext, ParseResult,
};
use crate::parser::core::parse_u32;

pub fn parse_alias_type(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<()> {
    let sort = parse_sort(ctx)?;
    match ctx.reader.read_exact_one()? {
        0x00 => {
            let _idx = parse_instance_local_idx(ctx)?;
            todo!();
        }
        0x02 => {
            let (_, ct) = parse_u32(ctx.reader)?;
            let (_, _idx) = parse_u32(ctx.reader)?;
            let _outer_scope = ctx.validator.outer_scope(ct);
            todo!()
        }
        _ => Err(ComponentParseError::InvalidSignature(format!(
            "Invalid alias type for instance decl: {sort:?}"
        ))),
    }
}
