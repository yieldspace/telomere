use crate::binary::BinaryReader;
use crate::component_model::{ComponentImport, ExternDesc, Relation};
use crate::parser::component_model::name::parse_import_name_dash;
use crate::parser::component_model::types::parse_externdesc;
use crate::parser::component_model::{ParseContext, ParseResult};
use tracing::trace;

pub fn parse_import(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<()> {
    let name = parse_import_name_dash(ctx)?;
    let desc = parse_externdesc(ctx)?;
    let (pid, register_id) = ctx
        .validator
        .scope_mut()
        .add_import_type(name, desc.clone())?;
    match desc {
        ExternDesc::Component(_) => {
            let (_, gid) = ctx
                .validator
                .scope_mut()
                .components
                .register_with_data(register_id, Relation::Import(pid));
            ctx.validator
                .scope_mut()
                .add_import(pid, ComponentImport::Component(gid))?;
        }
        ExternDesc::Instance(_) => {
            let (_, gid) = ctx
                .validator
                .scope_mut()
                .instances
                .register_with_data(register_id, Relation::Import(pid));
            ctx.validator
                .scope_mut()
                .add_import(pid, ComponentImport::Instance(gid))?;
        }
        ExternDesc::Eq(_) => {}
        ExternDesc::Sub => {}
        ExternDesc::Func(_) => {}
    }
    Ok(())
}
