use crate::binary::BinaryReader;
use crate::component::decoder::name::parse_export_name_dash;
use crate::component::decoder::sort::parse_sort_with_idx;
use crate::component::decoder::types::parse_externdesc;
use crate::component::decoder::types::validate_annotated_export;
use crate::component::decoder::{parse_option, ParseContext, ParseResult};
use crate::component::ir::types::GenericsReplaceDSL;
use crate::component::ir::{
    ComponentExport, ExternDesc, ParsedExportName, PlainName, Sort, StrongUnique,
};

use super::ComponentParseError;

pub fn parse_export(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<()> {
    tracing::trace!("parse_export");
    let name = parse_export_name_dash(ctx)?;
    let focus = ctx.validator.scope_mut();
    for existing in &focus.export_names {
        if existing.weak_eq(&name) {
            Err(ComponentParseError::InvalidExportName(format!(
                "export name `{}` conflicts with previous name `{}`",
                name.original, existing.original
            )))?;
        }
    }
    focus.export_names.push(name.clone());

    let si = parse_sort_with_idx(ctx)?;
    if matches!(si, Sort::Instance(_, _)) {
        ensure_concrete_surface_name(&name)?;
    }
    let desc = parse_option(ctx, parse_externdesc)?;
    if let Some(desc) = &desc {
        validate_annotated_export(ctx, &name, desc)?;
    } else if matches!(
        &name.parsed,
        ParsedExportName::Plain(
            PlainName::Constructor(_) | PlainName::Method(_, _) | PlainName::Static(_, _)
        )
    ) {
        match &si {
            Sort::Func(_, type_id) => {
                validate_annotated_export(ctx, &name, &ExternDesc::Func(*type_id))?;
            }
            _ => {
                return Err(ComponentParseError::TypeMismatch(
                    "annotated import/export is not a func".to_owned(),
                ));
            }
        }
    }
    match si {
        Sort::Module(idx, ty) => {
            ctx.state
                .scope_mut()
                .add_export(&name, ComponentExport::Module(idx));
            ctx.state.scope_mut().core_modules.register(idx);
            let focus = ctx.validator.scope_mut();
            focus.exports.insert(
                name.original.clone(),
                crate::component::decoder::validator::ExportInfo::CoreModule(ty.clone()),
            );
            focus
                .generics_replace_program
                .push(GenericsReplaceDSL::ExportCoreModule(
                    name.original.clone(),
                    ty,
                ));
            Ok(())
        }
        Sort::Type(type_id) => {
            let info = match desc {
                None => crate::component::decoder::validator::ExportInfo::TypeEq(type_id),
                Some(ExternDesc::Eq(id)) => {
                    ctx.validator
                        .get_type(type_id)?
                        .assert_subtype_of(ctx.validator.get_type(id)?, ctx.validator)?;
                    crate::component::decoder::validator::ExportInfo::TypeEq(id)
                }
                Some(ExternDesc::Sub) => {
                    if !ctx.validator.get_type(type_id)?.is_resource() {
                        Err(ComponentParseError::TypeMismatch(
                            "export kind mismatch".to_owned(),
                        ))?;
                    }
                    crate::component::decoder::validator::ExportInfo::TypeSub(type_id)
                }
                _ => Err(ComponentParseError::TypeMismatch(
                    "export kind mismatch".to_owned(),
                ))?,
            };
            let instr = match info {
                crate::component::decoder::validator::ExportInfo::TypeEq(id) => {
                    GenericsReplaceDSL::ExportTypeEq(name.original.clone(), id)
                }
                crate::component::decoder::validator::ExportInfo::TypeSub(_) => {
                    GenericsReplaceDSL::ExportTypeSub(name.original.clone(), type_id)
                }
                _ => unreachable!(),
            };
            let focus = ctx.validator.scope_mut();
            focus.type_indexes.add(type_id);
            focus.exports.insert(name.original.clone(), info);
            focus.generics_replace_program.push(instr);
            Ok(())
        }
        Sort::Component(idx, type_id) => {
            let export_type_id = match desc {
                None => type_id,
                Some(ExternDesc::Component(id)) => {
                    ctx.validator
                        .get_component_type(type_id)?
                        .assert_subtype_of(ctx.validator.get_component_type(id)?, ctx.validator)?;
                    id
                }
                _ => Err(ComponentParseError::TypeMismatch(
                    "export kind mismatch".to_owned(),
                ))?,
            };
            ctx.state
                .scope_mut()
                .add_export(&name, ComponentExport::Component(idx));
            let focus = ctx.validator.scope_mut();
            focus.component_indexes.add(export_type_id);
            focus.exports.insert(
                name.original.clone(),
                crate::component::decoder::validator::ExportInfo::Component(export_type_id),
            );
            focus
                .generics_replace_program
                .push(GenericsReplaceDSL::ExportComponent(
                    name.original.clone(),
                    export_type_id,
                ));
            Ok(())
        }
        Sort::Instance(idx, type_id) => {
            let export_type_id = match desc {
                None => type_id,
                Some(ExternDesc::Instance(id)) => {
                    ctx.validator
                        .get_instance_type(type_id)?
                        .assert_subtype_of(ctx.validator.get_instance_type(id)?, ctx.validator)?;
                    id
                }
                _ => Err(ComponentParseError::TypeMismatch(
                    "export kind mismatch".to_owned(),
                ))?,
            };
            ctx.state
                .scope_mut()
                .add_export(&name, ComponentExport::Instance(idx));
            ctx.state.scope_mut().instances.register(idx);
            let focus = ctx.validator.scope_mut();
            focus.exports.insert(
                name.original.clone(),
                crate::component::decoder::validator::ExportInfo::Instance(export_type_id),
            );
            focus
                .generics_replace_program
                .push(GenericsReplaceDSL::ExportInstance(
                    name.original.clone(),
                    export_type_id,
                ));
            focus.instance_indexes.add(export_type_id);
            Ok(())
        }
        Sort::Func(idx, type_id) => {
            let export_type_id = match desc {
                None => type_id,
                Some(ExternDesc::Func(id)) => {
                    ctx.validator
                        .get_func_type(type_id)?
                        .assert_subtype_of(ctx.validator.get_func_type(id)?, ctx.validator)?;
                    id
                }
                _ => Err(ComponentParseError::TypeMismatch(
                    "export kind mismatch".to_owned(),
                ))?,
            };
            ctx.state
                .scope_mut()
                .add_export(&name, ComponentExport::Func(idx));
            ctx.state.scope_mut().funcs.register(idx);
            let focus = ctx.validator.scope_mut();
            focus.exports.insert(
                name.original.clone(),
                crate::component::decoder::validator::ExportInfo::Func(export_type_id),
            );
            focus
                .generics_replace_program
                .push(GenericsReplaceDSL::ExportFunc(
                    name.original.clone(),
                    export_type_id,
                ));
            focus.func_indexes.add(export_type_id);
            Ok(())
        }
    }
}

fn ensure_concrete_surface_name(name: &crate::component::ir::ExportName) -> ParseResult<()> {
    let ParsedExportName::Plain(plain) = &name.parsed else {
        return Ok(());
    };
    for label in plain_labels(plain) {
        if !crate::component::decoder::name::is_kebab_label(label) {
            return Err(ComponentParseError::InvalidExportName(format!(
                "`{}` is not in kebab case",
                name.original
            )));
        }
    }
    Ok(())
}

fn plain_labels(plain: &PlainName) -> Vec<&str> {
    match plain {
        PlainName::Plain(label) | PlainName::Constructor(label) => vec![label.0.as_str()],
        PlainName::Method(resource, method) | PlainName::Static(resource, method) => {
            vec![resource.0.as_str(), method.0.as_str()]
        }
    }
}
