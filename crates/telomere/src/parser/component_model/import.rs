use crate::binary::BinaryReader;
use crate::component_model::types::{Generic, GenericBound};
use crate::component_model::{ComponentImport, ExternDesc, PlaceholderId, Relation, StrongUnique};
use crate::parser::component_model::name::parse_import_name_dash;
use crate::parser::component_model::types::parse_externdesc;
use crate::parser::component_model::{ParseContext, ParseResult};

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
                .register(Relation::Import(PlaceholderId::new(&name)));
            let focus = ctx.state.scope_mut();
            focus.add_import(&name, ComponentImport::Component);
            focus.components.register(global_idx);

            let focus = ctx.validator.scope_mut();
            focus.component_indexes.add(type_id);
            focus
                .imports
                .insert(name.original, Generic::new(GenericBound::Eq(type_id)));
        }
        ExternDesc::Instance(_) => {
            // todo register type
            ctx.state
                .scope_mut()
                .add_import(&name, ComponentImport::Instance);
            ctx.state
                .instance_store
                .register(Relation::Import(PlaceholderId::new(&name)));
        }
        ExternDesc::Eq(_) => {
            // todo register type
        }
        ExternDesc::Sub => {
            // todo register type
        }
        ExternDesc::Func(type_id) => {
            let global_idx = ctx
                .state
                .func_store
                .register(Relation::Import(PlaceholderId::new(&name)));

            let focus = ctx.state.scope_mut();
            focus.add_import(&name, ComponentImport::Func);
            focus.funcs.register(global_idx);

            let focus = ctx.validator.scope_mut();
            focus.func_indexes.add(type_id);
            focus
                .imports
                .insert(name.original, Generic::new(GenericBound::Eq(type_id)));
        }
    }
    Ok(())
}
