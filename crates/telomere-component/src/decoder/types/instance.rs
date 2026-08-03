use crate::decoder::types::parse_instance_decl;
use crate::decoder::{parse_vec_range, ComponentParseError, ParseContext, ParseResult};
use crate::ir::types::InstanceType;
use crate::support::binary::BinaryReader;

pub fn parse_instance_type(
    ctx: &mut ParseContext<impl BinaryReader>,
    depth: u32,
) -> ParseResult<InstanceType> {
    if depth > crate::MAX_COMPONENT_NESTING_DEPTH {
        return Err(ComponentParseError::NestingTooDeep {
            limit: crate::MAX_COMPONENT_NESTING_DEPTH,
        });
    }

    tracing::trace!("parse_instance_type");
    ctx.validator.push_nested_type_scope();
    for _ in parse_vec_range(ctx)? {
        parse_instance_decl(ctx, depth)?;
    }
    let instance_ty = ctx.validator.make_instance();
    ctx.validator.pop_scope();

    Ok(instance_ty)
}
