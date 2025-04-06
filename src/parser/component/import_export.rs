use crate::binary::{BinaryReader, Countable, Counter};
use crate::component_model::{ComponentExport, ComponentImport};
use crate::parser::component::context::ParseContext;
use crate::parser::component::id::parse_sort_idx;
use crate::parser::component::types::parse_externdesc;
use crate::parser::component::{parse_option, ComponentModelParserError};
use crate::parser::core::parse_name;
use crate::with_count;

type Result<R> = std::result::Result<R, ComponentModelParserError>;

pub fn parse_import(ctx: &mut ParseContext<impl BinaryReader>) -> Result<(usize, ComponentImport)> {
    let mut counter = Counter::new();
    let name = parse_import_name_dash(ctx)?.count(&mut counter);
    let ed = parse_externdesc(ctx)?.count(&mut counter);
    let import = ComponentImport { name, ed };
    Ok((counter.count(), import))
}

pub fn parse_export(ctx: &mut ParseContext<impl BinaryReader>) -> Result<(usize, ComponentExport)> {
    let mut counter = Counter::new();
    let name = parse_export_name_dash(ctx)?.count(&mut counter);
    let si = parse_sort_idx(ctx)?.count(&mut counter);
    let ed = parse_option(ctx, parse_externdesc)?.count(&mut counter);
    let export = ComponentExport { name, si, ed };
    Ok((counter.count(), export))
}

pub fn parse_import_name_dash(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> Result<(usize, String)> {
    Ok(with_count!(ctx.reader, {
        ComponentModelParserError::assert_magic(
            [ctx.reader.read_exact_one()?],
            [0x00],
            "import name",
        )?;
        // todo: check name
        let (_, name) = parse_name(ctx.reader)?;
        name
    }))
}

pub fn parse_export_name_dash(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> Result<(usize, String)> {
    Ok(with_count!(ctx.reader, {
        ComponentModelParserError::assert_magic(
            [ctx.reader.read_exact_one()?],
            [0x00],
            "export name",
        )?;
        // todo: check name
        let (_, name) = parse_name(ctx.reader)?;
        name
    }))
}
