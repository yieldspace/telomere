use crate::decoder::types::alias::parse_alias_type;
use crate::decoder::types::{parse_export_decl, parse_type};
use crate::decoder::{parse_core_type, DefinedCoreType};
use crate::decoder::{ComponentParseError, ParseContext, ParseResult};
use crate::support::binary::BinaryReader;

pub fn parse_instance_decl(
    ctx: &mut ParseContext<impl BinaryReader>,
    depth: u32,
) -> ParseResult<()> {
    _parse_instance_decl(ctx, None, depth)
}

pub fn _parse_instance_decl(
    ctx: &mut ParseContext<impl BinaryReader>,
    byte: Option<u8>,
    depth: u32,
) -> ParseResult<()> {
    tracing::trace!("_parse_instance_decl");
    let b = match byte {
        Some(b) => b,
        None => ctx.reader.read_exact_one()?,
    };
    match b {
        0x00 => {
            let (_, types) = parse_core_type(ctx)?;
            for DefinedCoreType {
                ty,
                is_final,
                identity,
            } in types
            {
                ctx.validator
                    .scope_mut()
                    .core_types
                    .add_with_records(ty, Some(is_final), identity);
            }
        }
        0x01 => {
            let t = parse_type(ctx, depth)?;
            let id = ctx.validator.new_type(t);
            // Types defined inside an instance or component type are
            // definitions like any other: the effective-size gate must
            // run here too, since the aggregate gate no longer covers
            // the value types a component/instance type contains.
            ctx.validator.validate_effective_type_size(id)?;
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
