use crate::binary::BinaryReader;
use crate::component_model::{Alias, AliasTarget};
use crate::parser::component::parser::context::ParseContext;
use crate::parser::component::parser::instance::parse_sort;
use crate::parser::component::parser::ComponentModelParserError;
use crate::parser::core::{parse_name, parse_u32};

type Result<R> = std::result::Result<R, ComponentModelParserError>;

pub fn parse_alias<R: BinaryReader>(ctx: &mut ParseContext<R>) -> Result<(usize, Alias)> {
    let (sort_len, sort) = parse_sort(ctx)?;
    match ctx.reader.read_exact_one()? {
        0x00 => {
            let (idx_len, idx) = parse_u32(ctx.reader)?;
            let (name_len, name) = parse_name(ctx.reader)?;
            Ok((
                sort_len + 1 + idx_len + name_len,
                Alias {
                    target: AliasTarget::Export(sort, name),
                },
            ))
        }
        0x01 => {
            let (idx_len, idx) = parse_u32(ctx.reader)?;
            let (name_len, name) = parse_name(ctx.reader)?;
            Ok((
                sort_len + 1 + idx_len + name_len,
                Alias {
                    target: AliasTarget::CoreExport(sort, name),
                },
            ))
        }
        0x02 => {
            let (ct_len, ct) = parse_u32(ctx.reader)?;
            let (idx_len, idx) = parse_u32(ctx.reader)?;
            Ok((
                sort_len + 1 + ct_len + idx_len,
                Alias {
                    target: AliasTarget::Outer(ct, sort),
                },
            ))
        }
        x => Err(ComponentModelParserError::InvalidAliasTarget(x)),
    }
}
