use crate::binary::BinaryReader;
use crate::common::ExportDesc;
use crate::component_model::types::GenericsReplaceDSL;
use crate::component_model::{ComponentExport, ExternDesc, Sort, StrongUnique};
use crate::parser::component_model::name::parse_export_name_dash;
use crate::parser::component_model::sort::parse_sort_with_idx;
use crate::parser::component_model::types::parse_externdesc;
use crate::parser::component_model::{parse_option, ParseContext, ParseResult};

use super::ComponentParseError;

pub fn parse_export(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<()> {
    tracing::trace!("parse_export");
    let name = parse_export_name_dash(ctx)?;
    let focus = ctx.validator.scope_mut();
    for existing in &focus.export_names {
        if existing.weak_eq(&name.parsed) {
            Err(ComponentParseError::InvalidExportName(
                "export is redundant defined".to_owned(),
            ))?;
        }
    }
    focus.export_names.push(name.parsed.clone());

    let si = parse_sort_with_idx(ctx)?;
    let desc = parse_option(ctx, parse_externdesc)?;
    let focus = ctx.validator.scope_mut();
    match si {
        Sort::Type(type_id) => {
            focus.type_indexes.add(type_id);
            let instr = match desc {
                None => GenericsReplaceDSL::ExportTypeEq(name.original.clone(), type_id),
                Some(ExternDesc::Eq(id)) => {
                    // TODO: validate desc and type_id match
                    GenericsReplaceDSL::ExportTypeEq(name.original.clone(), id)
                }
                Some(ExternDesc::Sub) => {
                    // TODO: type_id must references resource
                    GenericsReplaceDSL::ExportTypeSub(name.original.clone(), type_id)
                }
                _ => Err(ComponentParseError::TypeMismatch(
                    "export kind mismatch".to_owned(),
                ))?,
            };
            focus.generics_replace_program.push(instr);
            Ok(())
        }
        Sort::Component(_idx, type_id) => {
            ctx.state
                .scope_mut()
                .add_export(&name, ComponentExport::Component);
            focus.component_indexes.add(type_id);
            focus
                .generics_replace_program
                .push(GenericsReplaceDSL::ExportComponent(
                    name.original.clone(),
                    type_id,
                ));
            // TODO: validate desc
            Ok(())
        }
        Sort::Instance(idx, type_id) => {
            ctx.state
                .scope_mut()
                .add_export(&name, ComponentExport::Instance);
            ctx.state.scope_mut().instances.register(idx);
            focus
                .generics_replace_program
                .push(GenericsReplaceDSL::ExportInstance(
                    name.original.clone(),
                    type_id,
                ));
            focus.instance_indexes.add(type_id);
            // TODO: validate desc
            Ok(())
        }
        Sort::Func(idx, type_id) => {
            ctx.state
                .scope_mut()
                .add_export(&name, ComponentExport::Func);
            ctx.state.scope_mut().funcs.register(idx);
            focus
                .generics_replace_program
                .push(GenericsReplaceDSL::ExportFunc(
                    name.original.clone(),
                    type_id,
                ));
            focus.func_indexes.add(type_id);
            // TODO: validate desc
            Ok(())
        }
    }
}
