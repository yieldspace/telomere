use crate::binary::BinaryReader;
use crate::component_model::types::{
    ComponentExportType, Generic, GenericBound, ImportDecl, Type,
};
use crate::component_model::ExternDesc;
use crate::parser::component_model::name::{parse_export_name_dash, parse_import_name_dash};
use crate::parser::component_model::types::parse_externdesc;
use crate::parser::component_model::{ParseContext, ParseResult};

pub fn parse_import_decl(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<ImportDecl> {
    let name = parse_import_name_dash(ctx)?;
    let ed = parse_externdesc(ctx)?;
    Ok(ImportDecl::new(name, ed))
}

pub fn parse_export_decl<R: BinaryReader>(ctx: &mut ParseContext<R>) -> ParseResult<()> {
    let name = parse_export_name_dash(ctx)?;
    let desc = parse_externdesc(ctx)?;
    let export_ty = match &desc {
        ExternDesc::Component(id) => ComponentExportType::Component(*id),
        ExternDesc::Eq(id) => ComponentExportType::Type(*id),
        ExternDesc::Instance(id) => ComponentExportType::Instance(*id),
        ExternDesc::Func(id) => ComponentExportType::Type(*id), // FIXME: ?
        ExternDesc::Sub => {
            let id = ctx
                .validator
                .new_type(Type::Generic(Generic::new(GenericBound::Sub)));
            ComponentExportType::NewResource(id)
        }
    };
    ctx.validator
        .scope_mut()
        .exports
        .insert(name.original.clone(), export_ty);
    Ok(())
}
