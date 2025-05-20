use crate::binary::BinaryReader;
use crate::component_model::types::{ComponentType, ExportDecl, ImportDecl, InstanceDecl};
use crate::parser::component_model::types::instance_decl::_parse_instance_decl;
use crate::parser::component_model::types::interface::parse_import_decl;
use crate::parser::component_model::{parse_vec_range, ParseContext, ParseResult};

pub fn parse_component_type(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> ParseResult<ComponentType> {
    ctx.validator.push_scope();

    for _ in parse_vec_range(ctx)? {
        match ctx.reader.read_exact_one()? {
            0x03 => {
                let ImportDecl { name, desc } = parse_import_decl(ctx)?;
                // todo(type) add import type
            }
            x => {
                _parse_instance_decl(ctx, Some(x))?;
            }
        };
    }
    ctx.validator.pop_scope();

    // todo(type)
    Ok(ComponentType {
        imports: Default::default(),
        exports: Default::default(),
    })
}
