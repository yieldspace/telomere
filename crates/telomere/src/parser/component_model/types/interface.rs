use crate::binary::BinaryReader;
use crate::component_model::types::ImportDecl;
use crate::parser::component_model::name::{parse_export_name_dash, parse_import_name_dash};
use crate::parser::component_model::types::parse_externdesc;
use crate::parser::component_model::{ParseContext, ParseResult};

pub fn parse_import_decl(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<ImportDecl> {
    let name = parse_import_name_dash(ctx)?;
    let ed = parse_externdesc(ctx)?;
    Ok(ImportDecl::new(name, ed))
}

pub fn parse_export_decl<R: BinaryReader>(ctx: &mut ParseContext<R>) -> ParseResult<()> {
    let en = parse_export_name_dash(ctx)?;
    let ed = parse_externdesc(ctx)?;
    ctx.validator
        .with_scope(|scope| scope.add_export_type(en, ed))?;
    Ok(())
}
