use crate::binary::BinaryReader;
use crate::component_model::{ComponentExport, ExternDesc, Sort, StrongUnique};
use crate::parser::component_model::name::parse_export_name_dash;
use crate::parser::component_model::sort::parse_sort_with_idx;
use crate::parser::component_model::types::parse_externdesc;
use crate::parser::component_model::{parse_option, ParseContext, ParseResult};

use super::ComponentParseError;

pub fn parse_export(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<()> {
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
    let _ed = parse_option(ctx, parse_externdesc)?;
    match si {
        Sort::Type(id) => {
            // todo(type) register type
            Ok(())
        }
        Sort::Component(idx, tid) => {
            ctx.state
                .scope_mut()
                .add_export(&name, ComponentExport::Component);
            // todo(type) register type
            Ok(())
        }
        Sort::Instance(idx, tid) => {
            ctx.state
                .scope_mut()
                .add_export(&name, ComponentExport::Instance);
            ctx.state.scope_mut().instances.register(idx);
            // todo(type) register type
            Ok(())
        }
        Sort::Func(idx, _) => {
            ctx.state
                .scope_mut()
                .add_export(&name, ComponentExport::Func);
            ctx.state.scope_mut().funcs.register(idx);
            // todo(type) register type
            Ok(())
        }
    }
}
