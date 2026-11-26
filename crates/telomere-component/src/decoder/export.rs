use crate::decoder::name::{parse_export_name_dash, validate_name_attributes_sort};
use crate::decoder::sort::parse_sort_with_idx;
use crate::decoder::types::parse_externdesc;
use crate::decoder::types::validate_annotated_export;
use crate::decoder::{parse_option, ParseContext, ParseResult};
use crate::ir::types::{Generic, GenericBound, GenericsReplaceDSL, Type};
use crate::ir::{
    ComponentExport, ExternDesc, ParsedExportName, PlainName, PlainNameShape, Sort, StrongUnique,
};
use crate::support::binary::BinaryReader;

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
    validate_name_attributes_sort(matches!(si, Sort::Instance(_, _)), &name.attributes)
        .map_err(ComponentParseError::InvalidExportName)?;
    ensure_concrete_surface_name(&name)?;
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
                crate::decoder::validator::ExportInfo::CoreModule(ty.clone()),
            );
            // An export appends the exported item to its sort's index
            // space, so the validator's core module store must grow by
            // one entry as well (the state store above already does via
            // `register`); without it, a later reference to the
            // export-introduced index fails with `type idx not found`.
            focus.core_modules.add(ty.clone());
            focus
                .generics_replace_program
                .push(GenericsReplaceDSL::ExportCoreModule(
                    name.original.clone(),
                    ty,
                ));
            Ok(())
        }
        Sort::Type(type_id) => {
            // A `(sub resource)` ascription gives the export a fresh
            // abstract identity, distinct from the underlying resource
            // (and from any other ascription of it). Like a sub-bounded
            // import, the export therefore gets its own generic type id,
            // and that face id is registered consistently: the
            // generics-replace program keys the per-instantiation fresh
            // resource on it, `ExportInfo` carries it as the export's
            // type, and the type index space maps the export's index to
            // it -- so a later `(type $face)` reference to this export
            // denotes the face identity, not the underlying resource.
            let mut sub_face_id = None;
            let mut plain_face_id = None;
            let info = match desc {
                None => {
                    // A plain export introduces a new type index
                    // that denotes the exported type under the
                    // export's name; the type index passed *into*
                    // the export does not retroactively gain that
                    // name (external visibility). Mint an `(eq)`
                    // face for the export -- the analogue of the
                    // `(sub resource)` face below -- so references
                    // through the export's index stay distinct
                    // from references to the definition index.
                    // Consumers keep seeing the underlying type:
                    // the generics-replace program resolves
                    // `ExportTypeEq` through eq aliases (see
                    // `resolve_export_type_eq`).
                    let export_id = if matches!(
                        ctx.validator.get_type(type_id)?,
                        Type::Resource(_) | Type::DefVal(_) | Type::Generic(_)
                    ) {
                        let face_id = ctx
                            .validator
                            .new_type(Type::Generic(Generic::new(GenericBound::Eq(type_id))));
                        ctx.validator.note_plain_export_face(face_id, type_id);
                        plain_face_id = Some(face_id);
                        face_id
                    } else {
                        type_id
                    };
                    crate::decoder::validator::ExportInfo::TypeEq(export_id)
                }
                Some(ExternDesc::Eq(id)) => {
                    let item_ty = ctx.validator.get_type(type_id)?;
                    let ascribed_ty = ctx.validator.get_type(id)?;
                    // An `(eq ...)` ascription asserts that the
                    // exported type *is* the ascribed type, so resource
                    // references need nominal identity, not mere
                    // subtyping: the generic subtype relation accepts
                    // any two `Sub`-bounded generics (and any resource
                    // against a `Sub` generic) without comparing
                    // identities, which would let this export denote a
                    // different resource externally than its local type
                    // index denotes -- at the top level, and equally
                    // inside structural types (`own`/`borrow`, records,
                    // variants, lists, function types) or the entries
                    // of component/instance types built over the faces.
                    // The identity walk checks every resource-reference
                    // leaf; the subtype check below decides the rest.
                    item_ty.assert_same_resource_identities(ascribed_ty, ctx.validator)?;
                    item_ty.assert_subtype_of(ascribed_ty, ctx.validator)?;
                    crate::decoder::validator::ExportInfo::TypeEq(id)
                }
                Some(ExternDesc::Sub) => {
                    // The ascribed type may itself be an abstract
                    // resource face minted by an earlier sub-ascribed
                    // export and registered in the type index space
                    // under its name; a chained ascription over it is
                    // as valid as one over the concrete resource.
                    if !ctx
                        .validator
                        .get_type(type_id)?
                        .is_resource_or_face(ctx.validator)?
                    {
                        Err(ComponentParseError::TypeMismatch(
                            "export kind mismatch".to_owned(),
                        ))?;
                    }
                    let face_id = ctx
                        .validator
                        .new_type(Type::Generic(Generic::new(GenericBound::Sub)));
                    // The metadata keeps the face, but checks that
                    // assert identity with the exported resource, or
                    // that need the resource the face denotes, must
                    // still be able to reach the underlying type.
                    ctx.validator.note_sub_ascribed_face(face_id, type_id);
                    sub_face_id = Some(face_id);
                    crate::decoder::validator::ExportInfo::TypeSub(face_id)
                }
                _ => Err(ComponentParseError::TypeMismatch(
                    "export kind mismatch".to_owned(),
                ))?,
            };
            let instr = match info {
                crate::decoder::validator::ExportInfo::TypeEq(id) => {
                    GenericsReplaceDSL::ExportTypeEq(name.original.clone(), id)
                }
                crate::decoder::validator::ExportInfo::TypeSub(_) => {
                    GenericsReplaceDSL::ExportTypeSub(
                        name.original.clone(),
                        sub_face_id.expect("sub export mints a face type id"),
                    )
                }
                _ => unreachable!(),
            };
            let focus = ctx.validator.scope_mut();
            focus
                .type_indexes
                .add(sub_face_id.or(plain_face_id).unwrap_or(type_id));
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
            // An export appends the exported item to its sort's index
            // space: the state store must map the export-introduced
            // index to the same component (the validator's
            // `component_indexes` below already grows), mirroring the
            // instance/func/value arms.
            ctx.state.scope_mut().components.register(idx);
            let focus = ctx.validator.scope_mut();
            focus.component_indexes.add(export_type_id);
            focus.exports.insert(
                name.original.clone(),
                crate::decoder::validator::ExportInfo::Component(export_type_id),
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
                crate::decoder::validator::ExportInfo::Instance(export_type_id),
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
            ctx.state.scope_mut().add_export(
                &name,
                ComponentExport::Func {
                    idx,
                    type_id: export_type_id,
                },
            );
            ctx.state.scope_mut().funcs.register(idx);
            let focus = ctx.validator.scope_mut();
            focus.exports.insert(
                name.original.clone(),
                crate::decoder::validator::ExportInfo::Func(export_type_id),
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
        Sort::Value(idx, ty) => {
            let export_ty = match desc {
                None => ty,
                Some(ExternDesc::Value(declared)) => {
                    ty.assert_subtype_of(&declared, ctx.validator)?;
                    declared
                }
                _ => Err(ComponentParseError::TypeMismatch(
                    "export kind mismatch".to_owned(),
                ))?,
            };
            ctx.state.scope_mut().add_export(
                &name,
                ComponentExport::Value {
                    idx,
                    ty: export_ty.clone(),
                },
            );
            ctx.state.scope_mut().values.register(idx);
            let focus = ctx.validator.scope_mut();
            focus.exports.insert(
                name.original.clone(),
                crate::decoder::validator::ExportInfo::Value(export_ty.clone()),
            );
            focus
                .generics_replace_program
                .push(GenericsReplaceDSL::ExportValue(
                    name.original.clone(),
                    export_ty.clone(),
                ));
            focus.value_indexes.add(export_ty);
            Ok(())
        }
    }
}

fn ensure_concrete_surface_name(name: &crate::ir::ExportName) -> ParseResult<()> {
    let ParsedExportName::Plain(plain) = &name.parsed else {
        return Ok(());
    };
    for label in plain_labels(plain) {
        if !crate::decoder::name::is_kebab_label(label) {
            return Err(ComponentParseError::InvalidExportName(format!(
                "`{}` is not a valid extern name",
                name.original
            )));
        }
    }
    Ok(())
}

fn plain_labels(plain: &PlainName) -> Vec<&str> {
    match plain.shape() {
        PlainNameShape::Bare(label) | PlainNameShape::Constructor(label) => {
            vec![label.0.as_str()]
        }
        PlainNameShape::Dotted(resource, method) => {
            vec![resource.0.as_str(), method.0.as_str()]
        }
    }
}
