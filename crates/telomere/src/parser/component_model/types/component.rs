use std::collections::HashMap;

use crate::binary::BinaryReader;
use crate::component_model::types::{
    ComponentType, Generic, GenericBound, ImportDecl,
};
use crate::component_model::ExternDesc;
use crate::parser::component_model::types::instance_decl::_parse_instance_decl;
use crate::parser::component_model::types::interface::parse_import_decl;
use crate::parser::component_model::{
    parse_vec_range, ComponentParseError, ParseContext, ParseResult,
};

pub fn parse_component_type(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> ParseResult<ComponentType> {
    ctx.validator.push_scope();
    let mut imports = HashMap::new();
    let exports = HashMap::new();
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
                    _ => todo!(),
                };
                if imports.insert(name.original, Generic::new(bound)).is_some() {
                    Err(ComponentParseError::InvalidImportName(
                        "Duplicated name".to_owned(),
                    ))?;
                }
                // todo(type) add import type
            }
            x => {
                _parse_instance_decl(ctx, Some(x))?;
            }
        };
    }
    ctx.validator.pop_scope();

    Ok(ComponentType { imports, exports })
}
