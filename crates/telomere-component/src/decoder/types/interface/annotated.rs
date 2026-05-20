use crate::decoder::validator::ExportInfo;
use crate::decoder::{ParseContext, ParseResult};
use crate::ir::types::{ComponentImportType, DefValType, Generic, GenericBound, Type, ValType};
use crate::ir::{ParsedExportName, ParsedImportName, PlainName, TypeId};
use crate::support::binary::BinaryReader;

pub(crate) fn validate_annotated_import(
    ctx: &ParseContext<impl BinaryReader>,
    name: &crate::ir::ImportName,
    desc: &crate::ir::ExternDesc,
) -> ParseResult<()> {
    let ParsedImportName::Plain(plain) = &name.parsed else {
        return Ok(());
    };
    validate_annotated_plain_name(
        ctx,
        AnnotatedContext::Import,
        name.original.as_str(),
        plain,
        desc,
    )
}

pub(crate) fn validate_annotated_export(
    ctx: &ParseContext<impl BinaryReader>,
    name: &crate::ir::ExportName,
    desc: &crate::ir::ExternDesc,
) -> ParseResult<()> {
    let ParsedExportName::Plain(plain) = &name.parsed else {
        return Ok(());
    };
    validate_annotated_plain_name(
        ctx,
        AnnotatedContext::Export,
        name.original.as_str(),
        plain,
        desc,
    )
}

#[derive(Clone, Copy)]
enum AnnotatedContext {
    Import,
    Export,
}

fn validate_annotated_plain_name(
    ctx: &ParseContext<impl BinaryReader>,
    context: AnnotatedContext,
    original: &str,
    plain: &PlainName,
    desc: &crate::ir::ExternDesc,
) -> ParseResult<()> {
    let func_type_id = match desc {
        crate::ir::ExternDesc::Func(type_id) => *type_id,
        _ if matches!(plain, PlainName::Plain(_)) => return Ok(()),
        _ => {
            return Err(crate::decoder::ComponentParseError::TypeMismatch(
                "annotated import/export is not a func".to_owned(),
            ));
        }
    };
    let func_ty = ctx.validator.get_func_type(func_type_id)?;

    match plain {
        PlainName::Plain(_) => Ok(()),
        #[cfg(feature = "component-gated-feature-async")]
        PlainName::Async(_, _) => Ok(()),
        PlainName::Constructor(resource_name) => {
            let returned = constructor_resource_result(ctx, func_ty).ok_or_else(|| {
                crate::decoder::ComponentParseError::TypeMismatch(if func_ty.result.is_none() {
                    "should return one value".to_owned()
                } else if func_ty
                    .result
                    .as_ref()
                    .is_some_and(|result| is_result_type(ctx, result))
                {
                    "function should return `(own $T)` or `(result (own $T))`".to_owned()
                } else {
                    "should return `(own $T)`".to_owned()
                })
            })?;
            let Some(expected) = direct_resource_name(ctx, context, resource_name.0.as_str())?
            else {
                return Err(missing_resource_name_error(ctx, context, original));
            };
            ensure_same_resource(ctx, returned, expected, context)
        }
        PlainName::Method(resource_name, _) => {
            validate_method_annotation(ctx, context, original, func_ty, resource_name)
        }
        #[cfg(feature = "component-gated-feature-async")]
        PlainName::AsyncMethod(resource_name, _) => {
            validate_method_annotation(ctx, context, original, func_ty, resource_name)
        }
        PlainName::Static(resource_name, _) => {
            validate_static_annotation(ctx, context, resource_name)
        }
        #[cfg(feature = "component-gated-feature-async")]
        PlainName::AsyncStatic(resource_name, _) => {
            validate_static_annotation(ctx, context, resource_name)
        }
    }
}

fn validate_method_annotation(
    ctx: &ParseContext<impl BinaryReader>,
    context: AnnotatedContext,
    original: &str,
    func_ty: &crate::ir::types::FuncType,
    resource_name: &crate::ir::Label,
) -> ParseResult<()> {
    let Some(first_param_name) = func_ty.param_names.first() else {
        return Err(crate::decoder::ComponentParseError::TypeMismatch(
            "should have at least one argument".to_owned(),
        ));
    };
    if first_param_name.0 != "self" {
        return Err(crate::decoder::ComponentParseError::TypeMismatch(
            "should have a first argument called `self`".to_owned(),
        ));
    }
    let Some(first_param) = func_ty.params.first() else {
        return Err(crate::decoder::ComponentParseError::TypeMismatch(
            "should have at least one argument".to_owned(),
        ));
    };
    let borrowed = borrow_resource_param(ctx, first_param).ok_or_else(|| {
        crate::decoder::ComponentParseError::TypeMismatch(
            "should take a first argument of `(borrow $T)`".to_owned(),
        )
    })?;
    let Some(expected) = direct_resource_name(ctx, context, resource_name.0.as_str())? else {
        return Err(missing_resource_name_error(ctx, context, original));
    };
    ensure_same_resource(ctx, borrowed, expected, context)
}

fn validate_static_annotation(
    ctx: &ParseContext<impl BinaryReader>,
    context: AnnotatedContext,
    resource_name: &crate::ir::Label,
) -> ParseResult<()> {
    if direct_resource_name(ctx, context, resource_name.0.as_str())?.is_some() {
        Ok(())
    } else {
        Err(match context {
            AnnotatedContext::Import => crate::decoder::ComponentParseError::TypeMismatch(
                "static resource name is not known in this context".to_owned(),
            ),
            AnnotatedContext::Export => crate::decoder::ComponentParseError::TypeMismatch(
                "resource used in function does not have a name in this context".to_owned(),
            ),
        })
    }
}

fn missing_resource_name_error(
    ctx: &ParseContext<impl BinaryReader>,
    context: AnnotatedContext,
    original: &str,
) -> crate::decoder::ComponentParseError {
    match context {
        AnnotatedContext::Import => first_direct_resource_name(ctx, context)
            .map(|name| {
                crate::decoder::ComponentParseError::TypeMismatch(format!(
                    "function does not match expected resource name `{name}`"
                ))
            })
            .unwrap_or_else(|| {
                crate::decoder::ComponentParseError::InvalidImportName(format!(
                    "import name `{original}` is not valid"
                ))
            }),
        AnnotatedContext::Export => crate::decoder::ComponentParseError::TypeMismatch(
            "resource used in function does not have a name in this context".to_owned(),
        ),
    }
}

fn direct_resource_name(
    ctx: &ParseContext<impl BinaryReader>,
    context: AnnotatedContext,
    expected_name: &str,
) -> ParseResult<Option<TypeId>> {
    match context {
        AnnotatedContext::Import => {
            for name in &ctx.validator.scope().import_names {
                let ParsedImportName::Plain(PlainName::Plain(label)) = &name.parsed else {
                    continue;
                };
                if label.0 != expected_name {
                    continue;
                }
                let Some(ComponentImportType::Type { type_id, .. }) =
                    ctx.validator.scope().imports.get(&name.original)
                else {
                    continue;
                };
                if is_resource_name_type(ctx, *type_id)? {
                    return Ok(Some(*type_id));
                }
            }
            Ok(None)
        }
        AnnotatedContext::Export => {
            for name in &ctx.validator.scope().export_names {
                let ParsedExportName::Plain(PlainName::Plain(label)) = &name.parsed else {
                    continue;
                };
                if label.0 != expected_name {
                    continue;
                }
                let Some(info) = ctx.validator.scope().exports.get(&name.original) else {
                    continue;
                };
                let maybe_type_id = match info {
                    ExportInfo::TypeEq(type_id) | ExportInfo::TypeSub(type_id) => Some(*type_id),
                    ExportInfo::CoreModule(_)
                    | ExportInfo::Component(_)
                    | ExportInfo::Instance(_)
                    | ExportInfo::Func(_) => None,
                };
                if let Some(type_id) = maybe_type_id {
                    if is_resource_name_type(ctx, type_id)? {
                        return Ok(Some(type_id));
                    }
                }
            }
            Ok(None)
        }
    }
}

fn first_direct_resource_name(
    ctx: &ParseContext<impl BinaryReader>,
    context: AnnotatedContext,
) -> Option<String> {
    match context {
        AnnotatedContext::Import => ctx.validator.scope().import_names.iter().find_map(|name| {
            let ParsedImportName::Plain(PlainName::Plain(label)) = &name.parsed else {
                return None;
            };
            let Some(ComponentImportType::Type { type_id, .. }) =
                ctx.validator.scope().imports.get(&name.original)
            else {
                return None;
            };
            is_resource_name_type(ctx, *type_id)
                .ok()
                .filter(|is_resource| *is_resource)
                .map(|_| label.0.clone())
        }),
        AnnotatedContext::Export => ctx.validator.scope().export_names.iter().find_map(|name| {
            let ParsedExportName::Plain(PlainName::Plain(label)) = &name.parsed else {
                return None;
            };
            let info = ctx.validator.scope().exports.get(&name.original)?;
            let type_id = match info {
                ExportInfo::TypeEq(type_id) | ExportInfo::TypeSub(type_id) => *type_id,
                _ => return None,
            };
            is_resource_name_type(ctx, type_id)
                .ok()
                .filter(|is_resource| *is_resource)
                .map(|_| label.0.clone())
        }),
    }
}

fn is_resource_name_type(
    ctx: &ParseContext<impl BinaryReader>,
    type_id: TypeId,
) -> ParseResult<bool> {
    Ok(match ctx.validator.get_type(type_id)? {
        Type::Resource(_) => true,
        Type::Generic(Generic {
            bound: GenericBound::Sub,
            ..
        }) => true,
        Type::Generic(Generic {
            bound: GenericBound::Eq(inner),
            ..
        }) => is_resource_name_type(ctx, *inner)?,
        Type::DefVal(DefValType::Own(inner)) | Type::DefVal(DefValType::Borrow(inner)) => {
            is_resource_name_type(ctx, *inner)?
        }
        Type::DefVal(_) | Type::Func(_) | Type::Component(_) | Type::Instance(_) => false,
    })
}

fn constructor_resource_result(
    ctx: &ParseContext<impl BinaryReader>,
    func_ty: &crate::ir::types::FuncType,
) -> Option<TypeId> {
    let result = func_ty.result.as_ref()?;
    own_resource_type(ctx, result).or_else(|| result_ok_own_resource_type(ctx, result))
}

fn result_ok_own_resource_type(
    ctx: &ParseContext<impl BinaryReader>,
    val_ty: &ValType,
) -> Option<TypeId> {
    let ValType::Type(type_id) = val_ty else {
        return None;
    };
    let Type::DefVal(DefValType::Variant(cases)) = ctx.validator.get_type(*type_id).ok()? else {
        return None;
    };
    if cases.len() != 2 || cases[0].label.0 != "ok" || cases[1].label.0 != "err" {
        return None;
    }
    own_resource_type(ctx, cases[0].ty.as_ref()?)
}

fn is_result_type(ctx: &ParseContext<impl BinaryReader>, val_ty: &ValType) -> bool {
    let ValType::Type(type_id) = val_ty else {
        return false;
    };
    let Ok(Type::DefVal(DefValType::Variant(cases))) = ctx.validator.get_type(*type_id) else {
        return false;
    };
    cases.len() == 2 && cases[0].label.0 == "ok" && cases[1].label.0 == "err"
}

fn own_resource_type(ctx: &ParseContext<impl BinaryReader>, val_ty: &ValType) -> Option<TypeId> {
    let ValType::Type(type_id) = val_ty else {
        return None;
    };
    let Type::DefVal(DefValType::Own(resource)) = ctx.validator.get_type(*type_id).ok()? else {
        return None;
    };
    Some(*resource)
}

fn borrow_resource_param(
    ctx: &ParseContext<impl BinaryReader>,
    val_ty: &ValType,
) -> Option<TypeId> {
    let ValType::Type(type_id) = val_ty else {
        return None;
    };
    let Type::DefVal(DefValType::Borrow(resource)) = ctx.validator.get_type(*type_id).ok()? else {
        return None;
    };
    Some(*resource)
}

fn ensure_same_resource(
    ctx: &ParseContext<impl BinaryReader>,
    actual: TypeId,
    expected: TypeId,
    context: AnnotatedContext,
) -> ParseResult<()> {
    ctx.validator
        .get_type(actual)?
        .assert_subtype_of(ctx.validator.get_type(expected)?, ctx.validator)
        .map_err(|_| match context {
            AnnotatedContext::Import => crate::decoder::ComponentParseError::TypeMismatch(format!(
                "function does not match expected resource name `{}`",
                resource_name_for_type(ctx, expected)
            )),
            AnnotatedContext::Export => crate::decoder::ComponentParseError::TypeMismatch(
                "resource used in function does not have a name in this context".to_owned(),
            ),
        })
}

fn resource_name_for_type(ctx: &ParseContext<impl BinaryReader>, expected: TypeId) -> String {
    for name in &ctx.validator.scope().import_names {
        let ParsedImportName::Plain(PlainName::Plain(label)) = &name.parsed else {
            continue;
        };
        let Some(ComponentImportType::Type { type_id, .. }) =
            ctx.validator.scope().imports.get(&name.original)
        else {
            continue;
        };
        if *type_id == expected {
            return label.0.clone();
        }
    }
    for name in &ctx.validator.scope().export_names {
        let ParsedExportName::Plain(PlainName::Plain(label)) = &name.parsed else {
            continue;
        };
        let Some(info) = ctx.validator.scope().exports.get(&name.original) else {
            continue;
        };
        match info {
            ExportInfo::TypeEq(type_id) | ExportInfo::TypeSub(type_id) if *type_id == expected => {
                return label.0.clone();
            }
            _ => {}
        }
    }
    "<unknown>".to_owned()
}
