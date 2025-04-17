use crate::binary::BinaryReader;
use crate::component_model::{ComponentExport, ComponentImport};
use crate::parser::component_model::types::parse_externdesc;
use crate::parser::component_model::{
    parse_option, parse_sort_with_idx, ComponentParseError, ParseContext, SizedResult,
};
use crate::parser::core::parse_name;

pub fn parse_import(ctx: &mut ParseContext<impl BinaryReader>) -> SizedResult<()> {
    let start_count = ctx.reader.read_count();
    let (_, name) = parse_import_name_dash(ctx)?;
    let (_, ed) = parse_externdesc(ctx)?;
    let import = ComponentImport { name, ed };
    ctx.validator.add_import(import)?;
    Ok((ctx.reader.read_count() - start_count, ()))
}

pub fn parse_export(ctx: &mut ParseContext<impl BinaryReader>) -> SizedResult<()> {
    let start_count = ctx.reader.read_count();
    let (_, name) = parse_export_name_dash(ctx)?;
    let (_, si) = parse_sort_with_idx(ctx)?;
    let (_, ed) = parse_option(ctx, parse_externdesc)?;
    let export = ComponentExport {
        name,
        sort: si,
        desc: ed,
    };
    ctx.validator.add_export(export)?;
    Ok((ctx.reader.read_count() - start_count, ()))
}

pub fn parse_import_name_dash(ctx: &mut ParseContext<impl BinaryReader>) -> SizedResult<String> {
    ComponentParseError::assert_magic([ctx.reader.read_exact_one()?], [0x00], "import name")?;
    // todo: check name
    let (len, name) = parse_name(ctx.reader)?;
    Ok((len + 1, name))
}

pub fn parse_export_name_dash(ctx: &mut ParseContext<impl BinaryReader>) -> SizedResult<String> {
    ComponentParseError::assert_magic([ctx.reader.read_exact_one()?], [0x00], "export name")?;
    // todo: check name
    let (len, name) = parse_name(ctx.reader)?;
    Ok((len + 1, name))
}
