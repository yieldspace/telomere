use crate::binary::BinaryReader;
use crate::component_model::types::{Generic, GenericBound, Type};
use crate::component_model::{ComponentImport, ExternDesc, InstanceImport, Relation, StrongUnique};
use crate::parser::component_model::name::parse_import_name_dash;
use crate::parser::component_model::types::parse_externdesc;
use crate::parser::component_model::{ParseContext, ParseResult};
use crate::runtime::component_model::instantiate::InstantiateOp;
use super::ComponentParseError;

pub fn parse_import(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<()> {
    let name = parse_import_name_dash(ctx)?;
    let focus = ctx.validator.scope_mut();
    for existing in &focus.import_names {
        if existing.weak_eq(&name.parsed) {
            Err(ComponentParseError::InvalidImportName(
                "import is redundant defined".to_owned(),
            ))?;
        }
    }
    focus.import_names.push(name.parsed.clone());
    let desc = parse_externdesc(ctx)?;
    match desc {
        ExternDesc::Component(type_id) => {
            let global_idx = ctx
                .state
                .component_store
                .register(Relation::Import(name.original.clone()));
            let focus = ctx.state.scope_mut();
            focus.add_import(&name, ComponentImport::Component);
            focus.components.register(global_idx);
            focus.push_op(InstantiateOp::MapImport(Box::new(name.original.clone()), InstanceImport::Component(global_idx)));

            let focus = ctx.validator.scope_mut();
            focus.component_indexes.add(type_id);
            focus
                .imports
                .insert(name.original, Generic::new(GenericBound::Eq(type_id)));
        }
        ExternDesc::Instance(type_id) => {
            let ty = ctx.validator.get_type(type_id);
            tracing::trace!("ExternDesc::Instance: {:?}", ty);
            let global = ctx
                .state
                .instance_store
                .register(Relation::Import(name.original.clone()));
            let focus = ctx.state.scope_mut();

            focus.add_import(&name, ComponentImport::Instance);
            focus.instances.register(global);
            focus.push_op(InstantiateOp::MapImport(Box::new(name.original.clone()), InstanceImport::Instance(global)));

            let focus = ctx.validator.scope_mut();

            focus
                .imports
                .insert(name.original, Generic::new(GenericBound::Eq(type_id)));
            focus.instance_indexes.add(type_id);
        }
        ExternDesc::Eq(type_id) => {
            let focus = ctx.validator.scope_mut();

            focus
                .imports
                .insert(name.original, Generic::new(GenericBound::Eq(type_id)));
            focus.type_indexes.add(type_id);
        }
        ExternDesc::Sub => {
            let generic = Generic::new(GenericBound::Sub);
            let type_id = ctx.validator.new_type(Type::Generic(generic.clone()));
            let focus = ctx.validator.scope_mut();
            focus.imports.insert(name.original, generic);
            focus.type_indexes.add(type_id);
        }
        ExternDesc::Func(type_id) => {
            let global_idx = ctx
                .state
                .func_store
                .register(Relation::Import(name.original.clone()));

            let focus = ctx.state.scope_mut();
            focus.add_import(&name, ComponentImport::Func);
            focus.funcs.register(global_idx);
            focus.push_op(InstantiateOp::MapImport(Box::new(name.original.clone()), InstanceImport::Func(global_idx)));

            let focus = ctx.validator.scope_mut();
            focus.func_indexes.add(type_id);
            focus
                .imports
                .insert(name.original, Generic::new(GenericBound::Eq(type_id)));
        }
    }
    Ok(())
}
