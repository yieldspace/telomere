use crate::binary::{BinaryReader, Countable, Counter};
use crate::component_model::{Alias, AliasTarget};
use crate::parser::component::context::ParseContext;
use crate::parser::component::core::parse_core_instance_idx;
use crate::parser::component::error::ComponentModelParserError;
use crate::parser::component::id::parse_instance_idx;
use crate::parser::component::instance::parse_sort;
use crate::parser::core::{parse_name, parse_u32};

type Result<R> = std::result::Result<R, ComponentModelParserError>;

pub fn parse_alias<R: BinaryReader>(ctx: &mut ParseContext<R>) -> Result<(usize, Alias)> {
    let mut counter = Counter::new();
    let sort = parse_sort(ctx)?.count(&mut counter);
    match ctx.reader.read_exact_one()?.count(&mut counter) {
        0x00 => {
            let idx = parse_instance_idx(ctx)?.count(&mut counter);
            let name = parse_name(ctx.reader)?.count(&mut counter);
            Ok((
                counter.count(),
                Alias {
                    target: AliasTarget::Export(sort, idx, name),
                },
            ))
        }
        0x01 => {
            let idx = parse_core_instance_idx(ctx)?.count(&mut counter);
            let name = parse_name(ctx.reader)?.count(&mut counter);
            Ok((
                counter.count(),
                Alias {
                    target: AliasTarget::CoreExport(sort, idx, name),
                },
            ))
        }
        0x02 => {
            let ct = parse_u32(ctx.reader)?.count(&mut counter);
            let idx = parse_u32(ctx.reader)?.count(&mut counter);
            Ok((
                counter.count(),
                Alias {
                    target: AliasTarget::Outer(ct, sort, idx),
                },
            ))
        }
        x => Err(ComponentModelParserError::InvalidAliasTarget(x)),
    }
}
