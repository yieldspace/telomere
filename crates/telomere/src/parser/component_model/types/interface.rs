use crate::binary::BinaryReader;
use crate::component_model::types::{Generic, GenericBound, GenericsReplaceDSL, ImportDecl, Type};
use crate::parser::component_model::name::{parse_export_name_dash, parse_import_name_dash};
use crate::parser::component_model::types::parse_externdesc;
use crate::parser::component_model::validator::ExportInfo;
use crate::parser::component_model::{ParseContext, ParseResult};

pub fn parse_import_decl(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<ImportDecl> {
    let name = parse_import_name_dash(ctx)?;
    let ed = parse_externdesc(ctx)?;
    Ok(ImportDecl::new(name, ed))
}

pub fn parse_export_decl<R: BinaryReader>(ctx: &mut ParseContext<R>) -> ParseResult<()> {
    let name = parse_export_name_dash(ctx)?;
    let desc = parse_externdesc(ctx)?;
    match desc {
        crate::component_model::ExternDesc::Component(type_id) => {
            let focus = ctx.validator.scope_mut();
            focus
                .exports
                .insert(name.original.clone(), ExportInfo::Component(type_id));
            focus.component_indexes.add(type_id);
            focus
                .generics_replace_program
                .push(GenericsReplaceDSL::ExportComponent(
                    name.original.clone(),
                    type_id,
                ));
        }
        crate::component_model::ExternDesc::Instance(type_id) => {
            let focus = ctx.validator.scope_mut();

            focus
                .exports
                .insert(name.original.clone(), ExportInfo::Instance(type_id));
            focus.instance_indexes.add(type_id);
            focus
                .generics_replace_program
                .push(GenericsReplaceDSL::ExportInstance(
                    name.original.clone(),
                    type_id,
                ));
        }
        crate::component_model::ExternDesc::Eq(type_id) => {
            let focus = ctx.validator.scope_mut();

            focus
                .exports
                .insert(name.original.clone(), ExportInfo::TypeEq(type_id));
            focus.type_indexes.add(type_id);
            focus
                .generics_replace_program
                .push(GenericsReplaceDSL::ExportTypeEq(
                    name.original.clone(),
                    type_id,
                ));
        }
        crate::component_model::ExternDesc::Sub => {
            let type_id = ctx
                .validator
                .new_type(Type::Generic(Generic::new(GenericBound::Sub)));
            let focus = ctx.validator.scope_mut();
            focus
                .exports
                .insert(name.original.clone(), ExportInfo::TypeSub);
            focus.type_indexes.add(type_id);
            focus
                .generics_replace_program
                .push(GenericsReplaceDSL::ExportTypeSub(
                    name.original.clone(),
                    type_id,
                ));
        }
        crate::component_model::ExternDesc::Func(type_id) => {
            let focus = ctx.validator.scope_mut();

            focus
                .exports
                .insert(name.original.clone(), ExportInfo::Func(type_id));
            focus.func_indexes.add(type_id);
            focus
                .generics_replace_program
                .push(GenericsReplaceDSL::ExportFunc(
                    name.original.clone(),
                    type_id,
                ));
        }
    }

    Ok(())
}
