use crate::binary::BinaryReader;
use crate::parser::component::parser::context::ParseContext;
use crate::parser::component::parser::ComponentModelParserError;
use crate::parser::core::parse_name;
use crate::{assert_magic, with_count};

type Result<R> = std::result::Result<R, ComponentModelParserError>;

pub fn parse_import_name_dash(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> Result<(usize, String)> {
    Ok(with_count!(ctx.reader, {
        assert_magic!(
            ctx.reader.read_exact_one()?,
            0x00,
            ComponentModelParserError::InvalidImportNameMagic
        );
        // todo: check name
        let (_, name) = parse_name(ctx.reader)?;
        name
    }))
}

pub fn parse_export_name_dash(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> Result<(usize, String)> {
    Ok(with_count!(ctx.reader, {
        assert_magic!(
            ctx.reader.read_exact_one()?,
            0x00,
            ComponentModelParserError::InvalidImportNameMagic
        );
        // todo: check name
        let (_, name) = parse_name(ctx.reader)?;
        name
    }))
}
