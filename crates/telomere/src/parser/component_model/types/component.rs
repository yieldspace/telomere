use crate::binary::BinaryReader;
use crate::component_model::types::{ComponentType, ExportDecl, ImportDecl, InstanceDecl};
use crate::parser::component_model::types::instance_decl::_parse_instance_decl;
use crate::parser::component_model::types::interface::parse_import_decl;
use crate::parser::component_model::{parse_vec_range, ParseContext, ParseResult};

pub fn parse_component_type(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> ParseResult<ComponentType> {
    ctx.validator.new_scope();

    for _ in parse_vec_range(ctx)? {
        match ctx.reader.read_exact_one()? {
            0x03 => {
                let ImportDecl { name, desc } = parse_import_decl(ctx)?;
                ctx.validator.scope_mut().add_import_type(name, desc)?;
            }
            x => {
                _parse_instance_decl(ctx, Some(x))?;
            }
        };
    }

    let ty = ctx.validator.scope().make_component_type();
    ctx.validator.merge_types_into_parent();
    ctx.validator.pop_scope();

    Ok(ty)
}
