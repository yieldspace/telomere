use crate::decoder::name::parse_label_dash;
use crate::decoder::types::valtype::parse_valtype;
use crate::decoder::{parse_option, ComponentParseError, ParseContext, ParseResult};
use crate::ir::types::Case;
use crate::support::binary::BinaryReader;

pub fn parse_case(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<Case> {
    let l = parse_label_dash(ctx)?;
    let t = parse_option(ctx, parse_valtype)?;
    ComponentParseError::assert_magic([ctx.reader.read_exact_one()?], [0x00], "case")?;
    Ok(Case { label: l, ty: t })
}
