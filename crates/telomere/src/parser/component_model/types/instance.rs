
use crate::binary::BinaryReader;
use crate::component_model::types::InstanceType;
use crate::parser::component_model::types::parse_instance_decl;
use crate::parser::component_model::{parse_vec_range, ParseContext, ParseResult};

pub fn parse_instance_type(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<InstanceType> {
    ctx.validator.push_scope();
    for _ in parse_vec_range(ctx)? {
        parse_instance_decl(ctx)?;
    }
    let instance_ty =  ctx.validator.make_instance();
    
    ctx.validator.pop_scope();

    Ok(instance_ty)
}
