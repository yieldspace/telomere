use crate::binary::BinaryReader;
use crate::component_model::{Alias, AliasTarget};
use crate::parser::component::parser::instance::parse_sort;
use crate::parser::component::parser::ComponentModelParserError;
use crate::parser::core::{parse_name, parse_u32};

type Result<R> = std::result::Result<R, ComponentModelParserError>;

pub fn parse_alias<R: BinaryReader>(reader: &mut R) -> Result<(usize, Alias)> {
    let (sort_len, sort) = parse_sort(reader)?;
    match reader.read_exact_one()? {
        0x00 => {
            let (idx_len, idx) = parse_u32(reader)?;
            let (name_len, name) = parse_name(reader)?;
            Ok((
                sort_len + 1 + idx_len + name_len,
                Alias {
                    sort,
                    target: AliasTarget::Export(idx as usize, name),
                },
            ))
        }
        0x01 => {
            let (idx_len, idx) = parse_u32(reader)?;
            let (name_len, name) = parse_name(reader)?;
            Ok((
                sort_len + 1 + idx_len + name_len,
                Alias {
                    sort,
                    target: AliasTarget::CoreExport(idx as usize, name),
                },
            ))
        }
        0x02 => {
            let (ct_len, ct) = parse_u32(reader)?;
            let (idx_len, idx) = parse_u32(reader)?;
            Ok((
                sort_len + 1 + ct_len + idx_len,
                Alias {
                    sort,
                    target: AliasTarget::Outer(ct, idx as usize),
                },
            ))
        }
        x => Err(ComponentModelParserError::InvalidAliasTarget(x)),
    }
}
