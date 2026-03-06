use crate::binary::BinaryReader;
use crate::component::decoder::parse_core_type;
use crate::component::decoder::types::alias::parse_alias_type;
use crate::component::decoder::types::{parse_export_decl, parse_type};
use crate::component::decoder::{ComponentParseError, ParseContext, ParseResult};

pub fn parse_instance_decl(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<()> {
    _parse_instance_decl(ctx, None)
}

pub fn _parse_instance_decl(
    ctx: &mut ParseContext<impl BinaryReader>,
    byte: Option<u8>,
) -> ParseResult<()> {
    tracing::trace!("_parse_instance_decl");
    let b = match byte {
        Some(b) => b,
        None => ctx.reader.read_exact_one()?,
    };
    match b {
        0x00 => {
            let (_, ty) = parse_core_type(ctx)?;
            ctx.validator.scope_mut().core_types.add(ty);
        }
        0x01 => {
            let t = parse_type(ctx)?;
            let id = ctx.validator.new_type(t);
            ctx.validator.scope_mut().type_indexes.add(id);
        }
        0x02 => {
            parse_alias_type(ctx)?;
        }
        0x04 => {
            parse_export_decl(ctx)?;
        }
        x => {
            return Err(ComponentParseError::InvalidSignature(format!(
                "invalid instance decl opcode: {x}"
            )));
        }
    };
    Ok(())
}
