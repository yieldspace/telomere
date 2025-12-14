use crate::binary::BinaryReader;
use crate::component_model::types::{
    ComponentType, Generic, GenericBound, GenericsReplaceDSL, InstanceType, Type,
};
use crate::component_model::{
    Component, ComponentExport, ExportName, ImportName, Instance, InstanceExport, InstanceImport,
    Relation, Sort,
};
use crate::parser::component_model::name::{parse_export_name, parse_import_name};
use crate::parser::component_model::sort::parse_sort_with_idx;
use crate::parser::component_model::{
    parse_component_local_idx, ComponentParseError, ParseContext, ParseResult, SizedResult,
};
use crate::parser::core::parse_vec;
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
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
    let instance = Instance {
        component_idx: Some(component_gid),
        imports: args
            .iter()
            .filter_map(|(name, sort)| match sort {
                Sort::Component(idx, _) => {
                    (name.original.clone(), InstanceImport::Component(*idx)).into()
                }
                Sort::Instance(idx, _) => {
                    (name.original.clone(), InstanceImport::Instance(*idx)).into()
                }
                Sort::Func(idx, _) => (name.original.clone(), InstanceImport::Func(*idx)).into(),
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
    let mut component_exports = HashMap::new();
    let mut program = Vec::new();
    for (name, sort) in pairs {
        let op = match sort {
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
                component_exports.insert(name.original.clone(), ComponentExport::Func(global_idx));
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
    let exports = GenericsReplaceDSL::evaluate(&program, ctx.validator)?;

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
    ctx.validator.scope_mut().instance_indexes.add(id);

    Ok(())
}
