use crate::binary::BinaryReader;
use crate::component::decoder::types::instance_decl::_parse_instance_decl;
use crate::component::decoder::types::interface::parse_import_decl;
use crate::component::decoder::{parse_vec_range, ComponentParseError, ParseContext, ParseResult};
use crate::component::ir::types::{ComponentType, Generic, GenericBound, ImportDecl};
use crate::component::ir::ExternDesc;

pub fn parse_component_type(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> ParseResult<ComponentType> {
    ctx.validator.push_scope();

    for _ in parse_vec_range(ctx)? {
        match ctx.reader.read_exact_one()? {
            0x03 => {
                let ImportDecl { name, desc } = parse_import_decl(ctx)?;
                let bound = match desc {
                    ExternDesc::Sub => GenericBound::Sub,
                    ExternDesc::Eq(id) => GenericBound::Eq(id),
                    ExternDesc::Component(id) => GenericBound::Eq(id),
                    ExternDesc::Func(id) => GenericBound::Eq(id),
                    ExternDesc::Instance(id) => GenericBound::Eq(id),
                };
                let scope = ctx.validator.scope_mut();
                if scope
                    .imports
                    .insert(name.original, Generic::new(bound))
                    .is_some()
                {
                    Err(ComponentParseError::InvalidImportName(
                        "Duplicated name".to_owned(),
                    ))?;
                }
            }
            x => {
                _parse_instance_decl(ctx, Some(x))?;
            }
        };
    }
    let component_ty = ctx.validator.make_component();
    ctx.validator.pop_scope();

    Ok(component_ty)
}
