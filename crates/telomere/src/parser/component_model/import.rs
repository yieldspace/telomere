use crate::binary::BinaryReader;
use crate::component_model::{ComponentImport, ExternDesc, PlaceholderId, Relation};
use crate::parser::component_model::name::parse_import_name_dash;
use crate::parser::component_model::types::parse_externdesc;
use crate::parser::component_model::{ParseContext, ParseResult};
use tracing::trace;

pub fn parse_import(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<()> {
    let name = parse_import_name_dash(ctx)?;
    let desc = parse_externdesc(ctx)?;
    match desc {
        ExternDesc::Component(_) => {
            // todo register type
            ctx.state
                .scope_mut()
                .add_import(&name, ComponentImport::Component);
            ctx.state
                .component_store
                .register(Relation::Import(PlaceholderId::new(&name)));
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
        ExternDesc::Func(_) => {
            // todo register type
            ctx.state
                .scope_mut()
                .add_import(&name, ComponentImport::Func);
            ctx.state
                .func_store
                .register(Relation::Import(PlaceholderId::new(&name)));
        }
    }
    Ok(())
}
