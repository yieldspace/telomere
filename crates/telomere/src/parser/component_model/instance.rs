use crate::binary::BinaryReader;
use crate::component_model::types::{
    Generic, GenericBound, InstanceExportType, InstanceType, Type, GenericsReplaceDSL,
};
use crate::component_model::{
    CoreSort, ImportName, InstanceExport, Instance, InstanceImport, Relation, Sort,
};
use crate::parser::component_model::name::{parse_export_name, parse_import_name};
use crate::parser::component_model::sort::parse_sort_with_idx;
use crate::parser::component_model::{
    parse_component_local_idx, parse_vec_range, ComponentParseError, ParseContext, ParseResult,
    SizedResult,
};
use crate::parser::core::parse_vec;
use crate::runtime::component_model::instantiate::InstantiateOp;
use std::collections::{HashMap, HashSet};
use tracing::trace;

pub fn parse_instance(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<()> {
    trace!("parse instance");

    match ctx.reader.read_exact_one()? {
        0x00 => parse_instantiate(ctx),
        0x01 => parse_inlineexport(ctx),
        _ => panic!(),
    }
}

fn parse_instantiate(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<()> {
    trace!("parse instantiate");
    let component_lid = parse_component_local_idx(ctx)?;
    let (_, args) = parse_vec(ctx, |c| c.reader, parse_instantiate_arg)?;
    if args
        .iter()
        .map(|v| &v.0.original)
        .collect::<HashSet<_>>()
        .len()
        != args.len()
    {
        Err(ComponentParseError::TypeMismatch(
            "Duplicated target import name".to_owned(),
        ))?
    }
    let component_gid = ctx.state.scope().components.get(component_lid)?;
    let instance = Instance::Defined {
        component_idx: component_gid,
        imports: {
            let mut results = vec![];
            for (name, sort) in args.iter() {
                match sort {
                    Sort::Component(idx, _) => {
                        results.push((name.original.clone(), InstanceImport::Component(*idx)))
                    }
                    Sort::Instance(idx, _) => {
                        results.push((name.original.clone(), InstanceImport::Instance(*idx)))
                    }
                    Sort::Func(idx, _) => {
                        results.push((name.original.clone(), InstanceImport::Func(*idx)))
                    }
                    Sort::Type(_) => {}
                    Sort::Core(CoreSort::Module(idx, _)) => {
                        results.push((name.original.clone(), InstanceImport::CoreModule(*idx)))
                    }
                    _ => {
                        return Err(ComponentParseError::InvalidSignature(
                            "expected component, instance, func, or core module sort".to_owned(),
                        ));
                    }
                };
            }
            results.into_iter().collect()
        },
    };
    let instance_gid = ctx
        .state
        .instance_store
        .register(Relation::Defined(instance));
    ctx.state.scope_mut().instances.register(instance_gid);
    ctx.state
        .scope_mut()
        .push_op(InstantiateOp::Instantiate(instance_gid));
    let component_tid = ctx
        .validator
        .scope_mut()
        .component_indexes
        .get(component_lid)?;
    let component_ty = ctx.validator.get_component_type(component_tid)?;
    if component_ty.imports.len() > args.len() {
        Err(ComponentParseError::TypeMismatch(
            "insufficient instantiate arg len".to_owned(),
        ))?
    }
    for (name, sort) in &args {
        let component_def = component_ty.imports.get(&name.original).ok_or_else(|| {
            ComponentParseError::TypeMismatch(
                "The component does not have an import with that name".to_owned(),
            )
        })?;
        let b = ctx.validator.get_type(sort.type_id())?;

        match (b, component_def) {
            (
                Type::Resource(_),
                Generic {
                    id: _,
                    bound: GenericBound::Sub,
                },
            ) => (), // TODO: handle new resource type id
            (
                a,
                Generic {
                    id: _,
                    bound: GenericBound::Eq(b),
                },
            ) => {
                let b = ctx.validator.get_type(*b)?;
                tracing::trace!("instantiate_arg: {} {:?} {:?}", name.original, a, b);
                a.assert_subtype_of(b, ctx.validator)?;
            }
            _ => Err(ComponentParseError::TypeMismatch(
                "expected resource".to_owned(),
            ))?,
        }
    }
    let program = component_ty.generics_replacing_program.clone();
    let exports = GenericsReplaceDSL::evaluate(&program, ctx.validator)?;
    // TODO:
    let id = ctx
        .validator
        .new_type(Type::Instance(InstanceType { exports }));
    ctx.validator.scope_mut().instance_indexes.add(id);
    Ok(())
}

fn parse_instantiate_arg(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> SizedResult<(ImportName, Sort)> {
    let start_count = ctx.reader.read_count();
    trace!("parse instantiate arg");
    let name = parse_import_name(ctx)?;
    let sort = parse_sort_with_idx(ctx)?;
    Ok((ctx.reader.read_count() - start_count, (name, sort)))
}

fn parse_inlineexport(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<()> {
    trace!("parse inline export");
    let mut exports = HashMap::new();
    let mut export_types = HashMap::<String, InstanceExportType>::new();
    for _ in parse_vec_range(ctx)? {
        let name = parse_export_name(ctx)?;
        let sort = parse_sort_with_idx(ctx)?;
        export_types.insert(name.original.clone(), sort.clone().try_into()?);
        match sort {
            Sort::Core(CoreSort::Module(idx, _)) => {
                exports.insert(name.original, InstanceExport::CoreModule(idx));
            }
            Sort::Component(idx, _) => {
                exports.insert(name.original, InstanceExport::Component(idx));
            }
            Sort::Instance(idx, _) => {
                exports.insert(name.original, InstanceExport::Instance(idx));
            }
            Sort::Func(idx, _) => {
                exports.insert(name.original, InstanceExport::Func(idx));
            }
            Sort::Type(_) => {}
            _ => {
                return Err(ComponentParseError::InvalidSignature(
                    "Core sorts other than core module are not allowed".to_owned(),
                ))
            }
        }
    }
    let instance = Instance::InlineExport { exports };
    let instance_gid = ctx
        .state
        .instance_store
        .register(Relation::Defined(instance));
    ctx.state.scope_mut().instances.register(instance_gid);
    ctx.state
        .scope_mut()
        .push_op(InstantiateOp::InstantiateInlineExport(instance_gid));
    let id = ctx.validator.new_type(Type::Instance(InstanceType {
        exports: export_types,
    }));
    ctx.validator.scope_mut().instance_indexes.add(id);
    Ok(())
}
