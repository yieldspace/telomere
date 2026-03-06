use crate::binary::BinaryReader;
use crate::component::decoder::types::parse_instance_decl;
use crate::component::decoder::{parse_vec_range, ParseContext, ParseResult};
use crate::component::ir::types::InstanceType;

pub fn parse_instance_type(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<InstanceType> {
    tracing::trace!("parse_instance_type");
    ctx.validator.push_nested_type_scope();
    for _ in parse_vec_range(ctx)? {
        parse_instance_decl(ctx)?;
    }
    let instance_ty = ctx.validator.make_instance();
    ctx.validator.pop_scope();

    Ok(instance_ty)
}
