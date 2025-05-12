use crate::binary::BinaryReader;
use crate::component_model::{ComponentExport, ExternDesc, Sort};
use crate::parser::component_model::name::parse_export_name_dash;
use crate::parser::component_model::sort::parse_sort_with_idx;
use crate::parser::component_model::types::parse_externdesc;
use crate::parser::component_model::{parse_option, ParseContext, ParseResult};

pub fn parse_export(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<()> {
    let name = parse_export_name_dash(ctx)?;
    let si = parse_sort_with_idx(ctx)?;
    let ed = parse_option(ctx, parse_externdesc)?;
    match si {
        Sort::Type(idx) => {
            ctx.validator.with_scope(|scope| {
                let (pid, ty) = scope
                    .add_export_type(name, ed.unwrap_or_else(|| ExternDesc::Eq(idx)))?;
                scope.add_export(pid, ComponentExport::Type(idx))
            })?;
            Ok(())
        }
        Sort::Component(idx, tid) => {
            ctx.validator.with_scope(|scope| {
                let (pid, ty) = scope
                    .add_export_type(name, ed.unwrap_or_else(|| ExternDesc::Component(tid)))?;
                scope.add_export(pid, ComponentExport::Component(idx))
            })?;
            Ok(())
        }
        Sort::Instance(idx, tid) => {
            ctx.validator.with_scope(|scope| {
                let (pid, ty) =
                    scope.add_export_type(name, ed.unwrap_or_else(|| ExternDesc::Instance(tid)))?;
                scope.add_export(pid, ComponentExport::Instance(idx))
            })?;
            Ok(())
        }
    }
}
