use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use tracing::trace;
use crate::binary::BinaryReader;
use crate::component_model::{ImportName, Instance, Relation, Sort};
use crate::component_model::types::{ComponentType, Type};
use crate::parser::component_model::{parse_component_local_idx, ComponentParseError, ParseContext, ParseResult, SizedResult};
use crate::parser::component_model::name::parse_import_name;
use crate::parser::component_model::sort::parse_sort_with_idx;
use crate::parser::core::parse_vec;

pub fn parse_instance(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<()> {
    trace!("parse instance");

    match ctx.reader.read_exact_one()? {
        0x00 => {
            parse_instantiate(ctx)
        }
        0x01 => {
            parse_inlineexport(ctx)
        }
        _ => panic!()
    }
}

fn parse_instantiate(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<()> {
    trace!("parse instantiate");
    let component_lid = parse_component_local_idx(ctx)?;
    let id = ctx.validator.scope().components.get(component_lid)?;
    let ty: ComponentType = ctx.validator.scope_mut().get_type(id)?.clone().try_into().map_err(ComponentParseError::TypeMismatch)?;
    let (_, args) = parse_vec(ctx, |c| c.reader, parse_instantiate_arg)?;
    let args = args.into_iter().map(|(k, v)| ({
        let mut hasher = DefaultHasher::new();
        k.hash(&mut hasher);
        hasher.finish()
    }, v)).collect::<HashMap<_, _>>();
    if args.len() != ty.imports.len() {
        return Err(ComponentParseError::InvalidSignature(format!("Invalid number of imports: {} != {}", args.len(), ty.imports.len())));
    }
    let mut placeholders = HashMap::new();
    for (name, sort) in ty.imports.iter() {
        let Some(_) = args.get(&name.name_hash()) else {
            return Err(ComponentParseError::InvalidSignature(format!("Missing import: {name:?}")));
        };
        placeholders.insert(name.clone(), sort.get_type_id());
    }
    let (new_id, new_ty) = ctx.validator.scope_mut().complement_placeholder_type(id, placeholders)?;
    let new_ty: ComponentType = new_ty.try_into().map_err(ComponentParseError::TypeMismatch)?;
    let gid = ctx.validator.scope().components.get_global_idx(new_id)?;
    let data = ctx.validator.scope().components.get_data(new_id)?;

    let instance = Instance {
        component_idx: Some(gid),
        imports: Default::default(), // todo dataから
        exports: Default::default(),
    };
    let instance_type = new_ty.into();
    ctx.validator.with_scope(|scope| {
        let instance_id = scope.add_type(Type::Instance(instance_type));
        scope.instances.register_with_data(instance_id, Relation::Defined(instance));
    });

    Ok(())
}

fn parse_instantiate_arg(ctx: &mut ParseContext<impl BinaryReader>) -> SizedResult<(ImportName, Sort)> {
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
