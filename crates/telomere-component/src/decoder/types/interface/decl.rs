use crate::decoder::name::{is_kebab_label, parse_export_name_dash, parse_import_name_dash};
use crate::decoder::types::interface::annotated::{
    validate_annotated_export, validate_annotated_import,
};
use crate::decoder::types::parse_externdesc;
use crate::decoder::validator::ExportInfo;
use crate::decoder::{ParseContext, ParseResult};
use crate::ir::types::{Generic, GenericBound, GenericsReplaceDSL, ImportDecl, Type};
use crate::ir::StrongUnique;
use crate::support::binary::BinaryReader;

pub fn parse_import_decl(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<ImportDecl> {
    let name = parse_import_name_dash(ctx)?;
    if ctx
        .validator
        .scope()
        .import_names
        .iter()
        .any(|existing| existing.weak_eq(&name))
    {
        return Err(crate::decoder::ComponentParseError::InvalidImportName(
            format!(
                "import name `{}` conflicts with previous name",
                name.original
            ),
        ));
    }
    ctx.validator.scope_mut().import_names.push(name.clone());
    let ed = parse_externdesc(ctx)?;
    if !matches!(
        ed,
        crate::ir::ExternDesc::Eq(_) | crate::ir::ExternDesc::Sub
    ) {
        ensure_type_surface_name(&name.to_string())
            .map_err(crate::decoder::ComponentParseError::InvalidImportName)?;
    }
    validate_annotated_import(ctx, &name, &ed)?;
    Ok(ImportDecl::new(name, ed))
}

pub fn parse_export_decl<R: BinaryReader>(ctx: &mut ParseContext<R>) -> ParseResult<()> {
    let name = parse_export_name_dash(ctx)?;
    if ctx
        .validator
        .scope()
        .export_names
        .iter()
        .any(|existing| existing.weak_eq(&name))
    {
        return Err(crate::decoder::ComponentParseError::InvalidExportName(
            format!(
                "export name `{}` conflicts with previous name",
                name.original
            ),
        ));
    }
    ctx.validator.scope_mut().export_names.push(name.clone());
    let desc = parse_externdesc(ctx)?;
    if !matches!(
        desc,
        crate::ir::ExternDesc::Eq(_) | crate::ir::ExternDesc::Sub
    ) {
        ensure_type_surface_name(&name.to_string())
            .map_err(crate::decoder::ComponentParseError::InvalidExportName)?;
    }
    validate_annotated_export(ctx, &name, &desc)?;
    match desc {
        crate::ir::ExternDesc::Module(module_ty) => {
            let focus = ctx.validator.scope_mut();
            focus.exports.insert(
                name.original.clone(),
                ExportInfo::CoreModule(module_ty.clone()),
            );
            focus.core_modules.add(module_ty.clone());
            focus
                .generics_replace_program
                .push(GenericsReplaceDSL::ExportCoreModule(
                    name.original.clone(),
                    module_ty,
                ));
        }
        crate::ir::ExternDesc::Component(type_id) => {
            let focus = ctx.validator.scope_mut();
            focus
                .exports
                .insert(name.original.clone(), ExportInfo::Component(type_id));
            focus.component_indexes.add(type_id);
            focus
                .generics_replace_program
                .push(GenericsReplaceDSL::ExportComponent(
                    name.original.clone(),
                    type_id,
                ));
        }
        crate::ir::ExternDesc::Instance(type_id) => {
            let focus = ctx.validator.scope_mut();

            focus
                .exports
                .insert(name.original.clone(), ExportInfo::Instance(type_id));
            focus.instance_indexes.add(type_id);
            focus
                .generics_replace_program
                .push(GenericsReplaceDSL::ExportInstance(
                    name.original.clone(),
                    type_id,
                ));
        }
        crate::ir::ExternDesc::Eq(type_id) => {
            let focus = ctx.validator.scope_mut();

            focus
                .exports
                .insert(name.original.clone(), ExportInfo::TypeEq(type_id));
            focus.type_indexes.add(type_id);
            focus
                .generics_replace_program
                .push(GenericsReplaceDSL::ExportTypeEq(
                    name.original.clone(),
                    type_id,
                ));
        }
        crate::ir::ExternDesc::Sub => {
            let type_id = ctx
                .validator
                .new_type(Type::Generic(Generic::new(GenericBound::Sub)));
            let focus = ctx.validator.scope_mut();
            focus
                .exports
                .insert(name.original.clone(), ExportInfo::TypeSub(type_id));
            focus.type_indexes.add(type_id);
            focus
                .generics_replace_program
                .push(GenericsReplaceDSL::ExportTypeSub(
                    name.original.clone(),
                    type_id,
                ));
        }
        crate::ir::ExternDesc::Func(type_id) => {
            let focus = ctx.validator.scope_mut();

            focus
                .exports
                .insert(name.original.clone(), ExportInfo::Func(type_id));
            focus.func_indexes.add(type_id);
            focus
                .generics_replace_program
                .push(GenericsReplaceDSL::ExportFunc(
                    name.original.clone(),
                    type_id,
                ));
        }
        crate::ir::ExternDesc::Value(_) => Err(crate::decoder::ComponentParseError::Unsupported(
            "value exports are not supported".to_owned(),
        ))?,
    }

    Ok(())
}

fn ensure_type_surface_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("name cannot be empty".to_owned());
    }
    if name.contains(':') || name.contains('/') {
        return Ok(());
    }
    if let Some(label) = name.strip_prefix("[constructor]") {
        return ensure_label(label, name);
    }
    if let Some(rest) = name.strip_prefix("[method]") {
        let Some((resource, method)) = rest.split_once('.') else {
            return Err(format!("`{name}` is not in kebab case"));
        };
        ensure_label(resource, name)?;
        return ensure_label(method, name);
    }
    if let Some(rest) = name.strip_prefix("[static]") {
        let Some((resource, method)) = rest.split_once('.') else {
            return Err(format!("`{name}` is not in kebab case"));
        };
        ensure_label(resource, name)?;
        return ensure_label(method, name);
    }
    ensure_label(name, name)
}

fn ensure_label(label: &str, original: &str) -> Result<(), String> {
    if is_kebab_label(label) {
        Ok(())
    } else {
        Err(format!("`{original}` is not in kebab case"))
    }
}
