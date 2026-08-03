use crate::decoder::types::instance_decl::_parse_instance_decl;
use crate::decoder::types::interface::parse_import_decl;
use crate::decoder::{parse_vec_range, ComponentParseError, ParseContext, ParseResult};
use crate::ir::types::{ComponentImportType, ComponentType, Generic, GenericBound, ImportDecl};
use crate::ir::ExternDesc;
use crate::support::binary::BinaryReader;

pub fn parse_component_type(
    ctx: &mut ParseContext<impl BinaryReader>,
    depth: u32,
) -> ParseResult<ComponentType> {
    if depth > crate::MAX_COMPONENT_NESTING_DEPTH {
        return Err(ComponentParseError::NestingTooDeep {
            limit: crate::MAX_COMPONENT_NESTING_DEPTH,
        });
    }

    ctx.validator.push_type_scope();

    for _ in parse_vec_range(ctx)? {
        match ctx.reader.read_exact_one()? {
            0x03 => {
                let ImportDecl { name, desc } = parse_import_decl(ctx)?;
                let import_ty = match desc {
                    ExternDesc::Module(ty) => {
                        let scope = ctx.validator.scope_mut();
                        scope.core_modules.add(ty.clone());
                        ComponentImportType::CoreModule(ty)
                    }
                    ExternDesc::Sub => {
                        let generic = Generic::new(GenericBound::Sub);
                        let type_id = ctx
                            .validator
                            .new_type(crate::ir::types::Type::Generic(generic.clone()));
                        let scope = ctx.validator.scope_mut();
                        scope.type_indexes.add(type_id);
                        ComponentImportType::Type { type_id, generic }
                    }
                    ExternDesc::Eq(id) => {
                        ctx.validator.scope_mut().type_indexes.add(id);
                        ComponentImportType::Type {
                            type_id: id,
                            generic: Generic::new(GenericBound::Eq(id)),
                        }
                    }
                    ExternDesc::Component(id) => {
                        ctx.validator.scope_mut().component_indexes.add(id);
                        ComponentImportType::Type {
                            type_id: id,
                            generic: Generic::new(GenericBound::Eq(id)),
                        }
                    }
                    ExternDesc::Func(id) => {
                        ctx.validator.scope_mut().func_indexes.add(id);
                        ComponentImportType::Type {
                            type_id: id,
                            generic: Generic::new(GenericBound::Eq(id)),
                        }
                    }
                    ExternDesc::Instance(id) => {
                        ctx.validator.scope_mut().instance_indexes.add(id);
                        ComponentImportType::Type {
                            type_id: id,
                            generic: Generic::new(GenericBound::Eq(id)),
                        }
                    }
                    ExternDesc::Value(_) => Err(ComponentParseError::Unsupported(
                        "value imports are not supported".to_owned(),
                    ))?,
                };
                let scope = ctx.validator.scope_mut();
                if scope.imports.insert(name.original, import_ty).is_some() {
                    Err(ComponentParseError::InvalidImportName(
                        "Duplicated name".to_owned(),
                    ))?;
                }
            }
            x => {
                _parse_instance_decl(ctx, Some(x), depth)?;
            }
        };
    }
    let component_ty = ctx.validator.make_component();
    ctx.validator.validate_component_surface(&component_ty)?;
    ctx.validator.pop_scope();

    Ok(component_ty)
}
