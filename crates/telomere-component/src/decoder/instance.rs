use crate::decoder::name::{is_kebab_label, parse_export_name};
use crate::decoder::sort::parse_sort_with_idx;
use crate::decoder::types::validate_annotated_export;
use crate::decoder::{
    parse_component_local_idx, ComponentParseError, ParseContext, ParseResult, SizedResult,
    TransformContext,
};
use crate::ir::types::{
    ComponentImportType, Generic, GenericBound, GenericsReplaceDSL, InstanceType, Type,
};
use crate::ir::{
    Component, ComponentExport, ExportName, ExternDesc, Instance, InstanceImport, ParsedExportName,
    PlainName, Relation, Sort, StrongUnique,
};
use crate::support::binary::BinaryReader;
use crate::support::parser::core::{parse_name, parse_vec};
use std::collections::{HashMap, HashSet};
use tracing::trace;

pub fn parse_instance(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<()> {
    trace!("parse instance");

    match ctx.reader.read_exact_one()? {
        0x00 => parse_instantiate(ctx),
        0x01 => parse_inlineexport(ctx),
        x => Err(ComponentParseError::InvalidSignature(format!(
            "invalid instance opcode: {x}"
        ))),
    }
}

fn parse_instantiate(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<()> {
    trace!("parse instantiate");
    let component_lid = parse_component_local_idx(ctx)?;
    let (_, args) = parse_vec(ctx, |c| c.reader, parse_instantiate_arg)?;
    if args.iter().map(|v| &v.0).collect::<HashSet<_>>().len() != args.len() {
        Err(ComponentParseError::TypeMismatch(
            "Duplicated target import name".to_owned(),
        ))?
    }
    let component_gid = ctx.state.scope().components.get(component_lid)?;
    let instance = Instance {
        component_idx: Some(component_gid),
        imports: args
            .iter()
            .filter_map(|(name, sort)| match sort {
                Sort::Module(idx, _) => (name.clone(), InstanceImport::CoreModule(*idx)).into(),
                Sort::Component(idx, _) => (name.clone(), InstanceImport::Component(*idx)).into(),
                Sort::Instance(idx, _) => (name.clone(), InstanceImport::Instance(*idx)).into(),
                Sort::Func(idx, _) => (name.clone(), InstanceImport::Func(*idx)).into(),
                Sort::Type(_) => None,
            })
            .collect(),
    };
    let instance_gid = ctx
        .state
        .instance_store
        .register(Relation::Defined(instance));
    ctx.state.scope_mut().instances.register(instance_gid);
    let component_tid = ctx
        .validator
        .scope_mut()
        .component_indexes
        .get(component_lid)?;
    let component_ty = ctx.validator.get_component_type(component_tid)?.clone();
    let mut unified = ctx.validator.new_transform_context();
    if component_ty.imports.len() > args.len() {
        for expected_name in &component_ty.import_order {
            if !args.iter().any(|(name, _)| *name == *expected_name) {
                return Err(ComponentParseError::TypeMismatch(format!(
                    "missing import named `{expected_name}`"
                )));
            }
        }
    }
    for expected_name in &component_ty.import_order {
        let Some(component_def) = component_ty.imports.get(expected_name) else {
            continue;
        };
        let Some((_, sort)) = args.iter().find(|(name, _)| *name == *expected_name) else {
            return Err(ComponentParseError::TypeMismatch(format!(
                "missing import named `{expected_name}`"
            )));
        };
        match (sort, component_def) {
            (Sort::Module(_, actual), ComponentImportType::CoreModule(expected)) => {
                actual.assert_subtype_of(expected)?;
            }
            (
                sort,
                ComponentImportType::Type {
                    generic: component_def,
                    type_id: formal_type_id,
                },
            ) => {
                let Some(type_id) = sort.type_id() else {
                    Err(ComponentParseError::TypeMismatch(
                        "sort kind mismatch".to_owned(),
                    ))?
                };
                let actual = ctx.validator.get_type(type_id)?.clone();
                let formal_root = ctx.validator.get_type(*formal_type_id)?.clone();
                match (&actual, component_def) {
                    (
                        Type::Resource(_) | Type::Generic(_),
                        Generic {
                            id: _,
                            bound: GenericBound::Sub,
                        },
                    ) => {
                        unified.insert(*formal_type_id, type_id);
                    }
                    (
                        _,
                        Generic {
                            id: _,
                            bound: GenericBound::Eq(b),
                        },
                    ) => {
                        let expected_id = ctx.validator.instantiate_type_id(*b, &mut unified)?;
                        let expected = ctx.validator.get_type(expected_id)?.clone();
                        tracing::trace!(
                            "instantiate_arg: {} {:?} {:?}",
                            expected_name,
                            actual,
                            expected
                        );
                        if matches!(formal_root, Type::Component(_) | Type::Instance(_)) {
                            actual.assert_subtype_of(&expected, ctx.validator)?;
                            seed_nested_type_mappings(
                                *formal_type_id,
                                type_id,
                                ctx.validator,
                                &mut unified,
                            )?;
                        } else {
                            assert_strict_instantiation_match(&actual, &expected, ctx.validator)?;
                        }
                        unified.insert(*formal_type_id, type_id);
                    }
                    _ => Err(ComponentParseError::TypeMismatch(
                        "expected resource".to_owned(),
                    ))?,
                }
            }
            _ => Err(ComponentParseError::TypeMismatch(
                "import kind mismatch".to_owned(),
            ))?,
        }
    }
    let program = component_ty.generics_replacing_program.clone();
    let exports = GenericsReplaceDSL::evaluate(&program, ctx.validator, unified)?;
    // TODO:
    let id = ctx
        .validator
        .new_type(Type::Instance(InstanceType { exports }));
    ctx.validator.scope_mut().instance_indexes.add(id);
    Ok(())
}

fn parse_instantiate_arg(ctx: &mut ParseContext<impl BinaryReader>) -> SizedResult<(String, Sort)> {
    let start_count = ctx.reader.read_count();
    trace!("parse instantiate arg");
    let (_, name) = parse_name(ctx.reader)?;
    let sort = parse_sort_with_idx(ctx)?;

    Ok((ctx.reader.read_count() - start_count, (name, sort)))
}
fn name_sort(ctx: &mut ParseContext<impl BinaryReader>) -> SizedResult<(ExportName, Sort)> {
    let start_count = ctx.reader.read_count();
    trace!("parse name_sort");
    // FXIME: ここに0x00入れるの古い仕様な気がしている
    if ctx.reader.read_exact_one()? != 0x00 {
        return Err(ComponentParseError::EmptyVariant);
    }
    let name = parse_export_name(ctx)?;
    let sort = parse_sort_with_idx(ctx)?;
    Ok((ctx.reader.read_count() - start_count, (name, sort)))
}
fn parse_inlineexport(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<()> {
    let (_, pairs) = parse_vec(ctx, |e| e.reader, name_sort)?;
    let mut seen_names = Vec::<ExportName>::new();
    let mut component_exports = HashMap::new();
    let mut program = Vec::new();
    for (name, sort) in pairs {
        validate_inline_name(&name)?;
        if matches!(
            &name.parsed,
            ParsedExportName::Plain(
                PlainName::Constructor(_) | PlainName::Method(_, _) | PlainName::Static(_, _)
            )
        ) {
            match &sort {
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
        if let Some(previous) = seen_names.iter().find(|existing| existing.weak_eq(&name)) {
            return Err(ComponentParseError::InvalidExportName(format!(
                "export name `{}` conflicts with previous name `{}`",
                name.original, previous.original
            )));
        }
        seen_names.push(name.clone());
        let op = match sort {
            Sort::Module(global_idx, ty) => {
                component_exports
                    .insert(name.original.clone(), ComponentExport::Module(global_idx));
                GenericsReplaceDSL::ExportCoreModule(name.original.clone(), ty)
            }
            Sort::Component(global_idx, type_id) => {
                component_exports.insert(
                    name.original.clone(),
                    ComponentExport::Component(global_idx),
                );
                GenericsReplaceDSL::ExportComponent(name.original.clone(), type_id)
            }
            Sort::Instance(global_idx, type_id) => {
                component_exports
                    .insert(name.original.clone(), ComponentExport::Instance(global_idx));
                GenericsReplaceDSL::ExportInstance(name.original.clone(), type_id)
            }
            Sort::Func(global_idx, type_id) => {
                component_exports.insert(
                    name.original.clone(),
                    ComponentExport::Func {
                        idx: global_idx,
                        type_id,
                    },
                );
                GenericsReplaceDSL::ExportFunc(name.original.clone(), type_id)
            }
            Sort::Type(type_id) => GenericsReplaceDSL::ExportTypeEq(name.original.clone(), type_id),
        };
        program.push(op);
    }
    let component = Component {
        imports: Default::default(),
        exports: component_exports,
    };
    let component_gid = ctx
        .state
        .component_store
        .register(Relation::Defined(component));
    let exports = GenericsReplaceDSL::evaluate(
        &program,
        ctx.validator,
        ctx.validator.new_transform_context(),
    )?;

    let instance = Instance {
        component_idx: Some(component_gid),
        imports: Default::default(),
    };
    let instance_gid = ctx
        .state
        .instance_store
        .register(Relation::Defined(instance));
    ctx.state.scope_mut().instances.register(instance_gid);

    let id = ctx
        .validator
        .new_type(Type::Instance(InstanceType { exports }));
    ctx.validator.validate_effective_type_size(id)?;
    ctx.validator.scope_mut().instance_indexes.add(id);

    Ok(())
}

fn validate_inline_name(name: &ExportName) -> ParseResult<()> {
    let ParsedExportName::Plain(plain) = &name.parsed else {
        return Ok(());
    };
    match plain {
        PlainName::Plain(label) | PlainName::Constructor(label) => {
            ensure_inline_label(&label.0, &name.original)
        }
        PlainName::Method(resource, method) | PlainName::Static(resource, method) => {
            ensure_inline_label(&resource.0, &name.original)?;
            ensure_inline_label(&method.0, &name.original)
        }
    }
}

fn ensure_inline_label(label: &str, original: &str) -> ParseResult<()> {
    if is_kebab_label(label) {
        Ok(())
    } else {
        Err(ComponentParseError::InvalidExportName(format!(
            "`{original}` is not in kebab case"
        )))
    }
}

fn assert_strict_instantiation_match(
    actual: &Type,
    expected: &Type,
    validator: &crate::decoder::Validator,
) -> ParseResult<()> {
    use crate::ir::types::{DefValType, GenericBound, ValType};

    fn compare_valtype(
        actual: &ValType,
        expected: &ValType,
        validator: &crate::decoder::Validator,
    ) -> ParseResult<()> {
        match (actual, expected) {
            (ValType::Primitive(lhs), ValType::Primitive(rhs)) if lhs == rhs => Ok(()),
            (ValType::Type(lhs), ValType::Type(rhs)) => compare_type_ids(*lhs, *rhs, validator),
            _ => actual.assert_subtype_of(expected, validator),
        }
    }

    fn compare_type_ids(
        actual: crate::ir::TypeId,
        expected: crate::ir::TypeId,
        validator: &crate::decoder::Validator,
    ) -> ParseResult<()> {
        let actual = validator.get_type(actual)?;
        let expected = validator.get_type(expected)?;
        compare_types(actual, expected, validator)
    }

    fn compare_defval(
        actual: &DefValType,
        expected: &DefValType,
        validator: &crate::decoder::Validator,
    ) -> ParseResult<()> {
        match (actual, expected) {
            (DefValType::Primitive(lhs), DefValType::Primitive(rhs)) if lhs == rhs => Ok(()),
            (DefValType::Record(lhs), DefValType::Record(rhs)) => {
                if lhs.len() != rhs.len() {
                    return actual.assert_subtype_of(expected, validator);
                }
                for (lhs, rhs) in lhs.iter().zip(rhs.iter()) {
                    if lhs.label != rhs.label {
                        return actual.assert_subtype_of(expected, validator);
                    }
                    compare_valtype(&lhs.ty, &rhs.ty, validator)?;
                }
                Ok(())
            }
            (DefValType::Variant(lhs), DefValType::Variant(rhs)) => {
                if lhs.len() != rhs.len() {
                    return actual.assert_subtype_of(expected, validator);
                }
                for (lhs, rhs) in lhs.iter().zip(rhs.iter()) {
                    if lhs.label != rhs.label {
                        return actual.assert_subtype_of(expected, validator);
                    }
                    match (&lhs.ty, &rhs.ty) {
                        (Some(lhs), Some(rhs)) => compare_valtype(lhs, rhs, validator)?,
                        (None, None) => {}
                        _ => return actual.assert_subtype_of(expected, validator),
                    }
                }
                Ok(())
            }
            (DefValType::List(lhs, llen), DefValType::List(rhs, rlen)) => {
                if llen != rlen {
                    return actual.assert_subtype_of(expected, validator);
                }
                compare_valtype(lhs, rhs, validator)
            }
            (DefValType::Own(lhs), DefValType::Own(rhs))
            | (DefValType::Borrow(lhs), DefValType::Borrow(rhs)) => {
                compare_type_ids(*lhs, *rhs, validator)
            }
            _ => actual.assert_subtype_of(expected, validator),
        }
    }

    fn compare_types(
        actual: &Type,
        expected: &Type,
        validator: &crate::decoder::Validator,
    ) -> ParseResult<()> {
        match (actual, expected) {
            (
                Type::Generic(crate::ir::types::Generic {
                    bound: GenericBound::Eq(lhs),
                    ..
                }),
                other,
            ) => compare_types(validator.get_type(*lhs)?, other, validator),
            (
                other,
                Type::Generic(crate::ir::types::Generic {
                    bound: GenericBound::Eq(rhs),
                    ..
                }),
            ) => compare_types(other, validator.get_type(*rhs)?, validator),
            (
                Type::Generic(crate::ir::types::Generic {
                    id: lhs,
                    bound: GenericBound::Sub,
                }),
                Type::Generic(crate::ir::types::Generic {
                    id: rhs,
                    bound: GenericBound::Sub,
                }),
            ) if lhs == rhs => Ok(()),
            (Type::Resource(lhs), Type::Resource(rhs)) if lhs == rhs => Ok(()),
            (
                Type::Generic(crate::ir::types::Generic {
                    bound: GenericBound::Sub,
                    ..
                }),
                Type::Generic(crate::ir::types::Generic {
                    bound: GenericBound::Sub,
                    ..
                }),
            )
            | (Type::Resource(_), Type::Resource(_)) => Err(ComponentParseError::TypeMismatch(
                "resource types are not the same".to_owned(),
            )),
            (Type::DefVal(lhs), Type::DefVal(rhs)) => compare_defval(lhs, rhs, validator),
            (Type::Func(lhs), Type::Func(rhs)) => {
                if lhs.param_names != rhs.param_names || lhs.params.len() != rhs.params.len() {
                    return lhs.assert_subtype_of(rhs, validator);
                }
                for (lhs, rhs) in lhs.params.iter().zip(rhs.params.iter()) {
                    compare_valtype(lhs, rhs, validator)?;
                }
                match (&lhs.result, &rhs.result) {
                    (Some(lhs), Some(rhs)) => compare_valtype(lhs, rhs, validator),
                    (None, None) => Ok(()),
                    _ => lhs.assert_subtype_of(rhs, validator),
                }
            }
            _ => actual.assert_subtype_of(expected, validator),
        }
    }

    compare_types(actual, expected, validator)
}

fn seed_nested_type_mappings(
    formal_type_id: crate::ir::TypeId,
    actual_type_id: crate::ir::TypeId,
    validator: &mut crate::decoder::Validator,
    unified: &mut TransformContext,
) -> ParseResult<()> {
    use crate::ir::types::{
        ComponentExportType, ComponentImportType, DefValType, InstanceExportType, Type, ValType,
    };

    fn seed_valtype(
        actual: &ValType,
        formal: &ValType,
        validator: &mut crate::decoder::Validator,
        unified: &mut TransformContext,
    ) -> ParseResult<()> {
        match (actual, formal) {
            (ValType::Type(actual), ValType::Type(formal)) => {
                seed_type(*formal, *actual, validator, unified)
            }
            _ => Ok(()),
        }
    }

    fn seed_type(
        formal: crate::ir::TypeId,
        actual: crate::ir::TypeId,
        validator: &mut crate::decoder::Validator,
        unified: &mut TransformContext,
    ) -> ParseResult<()> {
        if unified.get(formal) == Some(actual) {
            return Ok(());
        }
        unified.insert(formal, actual);

        let formal_ty = validator.get_type(formal)?.clone();
        let actual_ty = validator.get_type(actual)?.clone();
        match (formal_ty, actual_ty) {
            (
                Type::DefVal(DefValType::Record(formal)),
                Type::DefVal(DefValType::Record(actual)),
            ) => {
                for (formal, actual) in formal.iter().zip(actual.iter()) {
                    seed_valtype(&actual.ty, &formal.ty, validator, unified)?;
                }
            }
            (
                Type::DefVal(DefValType::Variant(formal)),
                Type::DefVal(DefValType::Variant(actual)),
            ) => {
                for (formal, actual) in formal.iter().zip(actual.iter()) {
                    if let (Some(formal), Some(actual)) = (&formal.ty, &actual.ty) {
                        seed_valtype(actual, formal, validator, unified)?;
                    }
                }
            }
            (
                Type::DefVal(DefValType::List(formal, _)),
                Type::DefVal(DefValType::List(actual, _)),
            ) => {
                seed_valtype(&actual, &formal, validator, unified)?;
            }
            (Type::DefVal(DefValType::Own(formal)), Type::DefVal(DefValType::Own(actual)))
            | (
                Type::DefVal(DefValType::Borrow(formal)),
                Type::DefVal(DefValType::Borrow(actual)),
            ) => {
                seed_type(formal, actual, validator, unified)?;
            }
            (Type::Func(formal), Type::Func(actual)) => {
                for (formal, actual) in formal.params.iter().zip(actual.params.iter()) {
                    seed_valtype(actual, formal, validator, unified)?;
                }
                if let (Some(formal), Some(actual)) = (&formal.result, &actual.result) {
                    seed_valtype(actual, formal, validator, unified)?;
                }
            }
            (Type::Instance(formal), Type::Instance(actual)) => {
                for (name, formal_export) in &formal.exports {
                    let Some(actual_export) = actual.exports.get(name) else {
                        continue;
                    };
                    match (formal_export, actual_export) {
                        (InstanceExportType::Type(formal), InstanceExportType::Type(actual))
                        | (InstanceExportType::Func(formal), InstanceExportType::Func(actual))
                        | (
                            InstanceExportType::Component(formal),
                            InstanceExportType::Component(actual),
                        )
                        | (
                            InstanceExportType::Instance(formal),
                            InstanceExportType::Instance(actual),
                        ) => {
                            seed_type(*formal, *actual, validator, unified)?;
                        }
                        _ => {}
                    }
                }
            }
            (Type::Component(formal), Type::Component(actual)) => {
                for (name, formal_import) in &formal.imports {
                    let Some(actual_import) = actual.imports.get(name) else {
                        continue;
                    };
                    if let (
                        ComponentImportType::Type {
                            type_id: formal, ..
                        },
                        ComponentImportType::Type {
                            type_id: actual, ..
                        },
                    ) = (formal_import, actual_import)
                    {
                        seed_type(*formal, *actual, validator, unified)?;
                    }
                }
                for (name, formal_export) in &formal.exports {
                    let Some(actual_export) = actual.exports.get(name) else {
                        continue;
                    };
                    match (formal_export, actual_export) {
                        (ComponentExportType::Type(formal), ComponentExportType::Type(actual))
                        | (ComponentExportType::Func(formal), ComponentExportType::Func(actual))
                        | (
                            ComponentExportType::Component(formal),
                            ComponentExportType::Component(actual),
                        )
                        | (
                            ComponentExportType::Instance(formal),
                            ComponentExportType::Instance(actual),
                        ) => {
                            seed_type(*formal, *actual, validator, unified)?;
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    seed_type(formal_type_id, actual_type_id, validator, unified)
}
