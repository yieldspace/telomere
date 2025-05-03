use crate::binary::BinaryReader;
use crate::component_model::{ComponentExport, CoreSortWithIdx, ExportName, SortWithIdx};
use crate::parser::component_model::name::parse_export_name_dash;
use crate::parser::component_model::{
    parse_externdesc, parse_option, parse_sort_with_idx, ComponentParseError, ParseContext,
    ParseResult, SizedResult,
};
use crate::parser::core::parse_name;

pub fn parse_export(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> ParseResult<(ExportName, ComponentExport)> {
    let (_, name) = parse_export_name_dash(ctx)?;
    let (_, si) = parse_sort_with_idx(ctx)?;
    let ed = parse_option(ctx, parse_externdesc)?;
    let export = match si {
        SortWithIdx::Core(CoreSortWithIdx::Module(idx, ty)) => {
            let ty = if let Some(ed) = ed {
                // todo: check ed is super type of ty
                ed.try_into()?
            } else {
                ty
            };
            ComponentExport::CoreModule(ty, idx)
        }
        SortWithIdx::Func(idx, ty) => {
            let ty = if let Some(ed) = ed {
                // todo: check ed is super type of ty
                ed.try_into()?
            } else {
                ty
            };
            ComponentExport::Func(ty, idx)
        }
        SortWithIdx::Type(ty) => ComponentExport::Type(ty),
        SortWithIdx::Component(idx, ty) => {
            let ty = if let Some(ed) = ed {
                // todo: check ed is super type of ty
                ed.try_into()?
            } else {
                ty
            };
            ComponentExport::Component(ty, idx)
        }
        SortWithIdx::Instance(idx, ty) => {
            let ty = if let Some(ed) = ed {
                // todo: check ed is super type of ty
                ed.try_into()?
            } else {
                ty
            };
            ComponentExport::Instance(ty, idx)
        }
        _ => {
            return Err(ComponentParseError::InvalidSignature(format!(
                "Invalid core type for export: {si:?}"
            )));
        }
    };
    Ok((name, export))
}
