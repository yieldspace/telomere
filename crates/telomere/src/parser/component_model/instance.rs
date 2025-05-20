use crate::binary::BinaryReader;
use crate::component_model::types::{
    ComponentExportType, ComponentType, Generic, GenericBound, Type,
};
use crate::component_model::{ImportName, Instance, InstanceImport, PlaceholderId, Relation, Sort};
use crate::parser::component_model::name::parse_import_name;
use crate::parser::component_model::sort::parse_sort_with_idx;
use crate::parser::component_model::{
    parse_component_local_idx, ComponentParseError, ParseContext, ParseResult, SizedResult,
};
use crate::parser::core::parse_vec;
use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
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
                    (PlaceholderId::new(name), InstanceImport::Component(*idx)).into()
                }
                Sort::Instance(idx, _) => {
                    (PlaceholderId::new(name), InstanceImport::Instance(*idx)).into()
                }
                Sort::Func(idx, _) => (PlaceholderId::new(name), InstanceImport::Func(*idx)).into(),
                Sort::Type(_) => None,
            })
            .collect(),
        exports: Default::default(),
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
                a.assert_subtype_of(
                    ctx.validator.get_type(*b)?,
                    ctx.validator,
                )?;
            }
            _ => Err(ComponentParseError::TypeMismatch("expected resource".to_owned()))?,
        }
    }
    // todo check type and generics, create new instance type

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
    todo!();
    Ok(())
}
