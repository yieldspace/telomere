mod dependency;
mod interface;
mod plain;

use crate::decoder::{ComponentParseError, ParseContext, ParseResult};
use crate::ir::{ExportName, ImportName, Label, ParsedExportName, ParsedImportName};
use crate::support::binary::BinaryReader;
use crate::support::parser::core::parse_name;
use tracing::trace;

use dependency::{parse_dep_name_string, parse_hash_name_string, parse_url_name_string};
use interface::parse_interface_name_string;
pub(crate) use plain::is_kebab_label;
use plain::{parse_label, parse_plain_name_string};

pub fn parse_import_name_dash(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> ParseResult<ImportName> {
    match ctx.reader.read_exact_one()? {
        0x00 | 0x01 => {}
        magic => ComponentParseError::assert_magic([magic], [0x00], "import name")?,
    }
    parse_import_name(ctx)
}

pub fn parse_export_name_dash(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> ParseResult<ExportName> {
    trace!("parse_export_name_dash");
    match ctx.reader.read_exact_one()? {
        0x00 | 0x01 => {}
        magic => ComponentParseError::assert_magic([magic], [0x00], "export name")?,
    }
    parse_export_name(ctx)
}

pub fn parse_export_name(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<ExportName> {
    trace!("parse_export_name");
    let (_, name) = parse_name(ctx.reader)?;
    if name.starts_with("relative-url=") {
        return Err(ComponentParseError::InvalidExportName(format!(
            "`{name}` is not a valid extern name"
        )));
    }
    if let Some(parsed) = parse_plain_name_string(name.as_str())? {
        return Ok(ExportName {
            original: name,
            parsed: ParsedExportName::Plain(parsed),
        });
    }
    if let Some(parsed) = parse_interface_name_string(name.as_str())
        .map_err(ComponentParseError::InvalidExportName)?
    {
        return Ok(ExportName {
            original: name,
            parsed: ParsedExportName::Interface(parsed),
        });
    }
    Err(ComponentParseError::InvalidExportName(format!(
        "Invalid export name: `{}`",
        name
    )))
}

pub fn parse_import_name(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<ImportName> {
    trace!("parse_import_name");
    let (_, name) = parse_name(ctx.reader)?;

    if name.starts_with("relative-url=") {
        return Err(ComponentParseError::InvalidImportName(format!(
            "`{name}` is not a valid extern name"
        )));
    }
    if let Some(parsed) = parse_plain_name_string(name.as_str())? {
        return Ok(ImportName {
            original: name,
            parsed: ParsedImportName::Plain(parsed),
        });
    }
    if let Some(parsed) =
        parse_dep_name_string(name.as_str()).map_err(ComponentParseError::InvalidImportName)?
    {
        return Ok(ImportName {
            original: name,
            parsed: ParsedImportName::Dependency(parsed),
        });
    }
    if let Some(parsed) =
        parse_url_name_string(name.as_str()).map_err(ComponentParseError::InvalidImportName)?
    {
        return Ok(ImportName {
            original: name,
            parsed: ParsedImportName::Url(parsed),
        });
    }
    if let Some(parsed) =
        parse_hash_name_string(name.as_str()).map_err(ComponentParseError::InvalidImportName)?
    {
        return Ok(ImportName {
            original: name,
            parsed: ParsedImportName::Hash(parsed),
        });
    }
    if let Some(parsed) = parse_interface_name_string(name.as_str())
        .map_err(ComponentParseError::InvalidImportName)?
    {
        return Ok(ImportName {
            original: name,
            parsed: ParsedImportName::Interface(parsed),
        });
    }
    Err(ComponentParseError::InvalidImportName(format!(
        "Invalid import name: `{}`",
        name
    )))
}

pub fn parse_label_dash(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<Label> {
    let (_, name) = parse_name(ctx.reader)?;
    parse_label(&name).map_err(ComponentParseError::InvalidLabel)
}

#[cfg(test)]
mod tests {
    use super::dependency::parse_package_path;
    use super::interface::parse_interface_name_string;
    use super::plain::parse_plain_name_string;
    use crate::ir::{InterfaceName, Label, PlainName};
    use semver::Version;

    #[test]
    fn test_plain_name() {
        let name = "foo";
        let export_name = parse_plain_name_string(name).unwrap();
        assert_eq!(export_name, Some(PlainName::Plain(Label::new(name))));
    }

    #[test]
    fn test_export_name_constructor() {
        let name = "[constructor]foo";
        let export_name = parse_plain_name_string(name).unwrap();
        assert_eq!(
            export_name,
            Some(PlainName::Constructor(Label::new("foo".to_string())))
        );
    }

    #[test]
    fn test_export_name_method() {
        let name = "[method]foo.bar";
        let export_name = parse_plain_name_string(name).unwrap();
        assert_eq!(
            export_name,
            Some(PlainName::Method(
                Label::new("foo".to_string()),
                Label::new("bar".to_string())
            ))
        );
    }

    #[test]
    fn test_export_name_static() {
        let name = "[static]foo.bar";
        let export_name = parse_plain_name_string(name).unwrap();
        assert_eq!(
            export_name,
            Some(PlainName::Static(
                Label::new("foo".to_string()),
                Label::new("bar".to_string())
            ))
        );
    }

    #[cfg(feature = "component-gated-feature-async")]
    #[test]
    fn test_export_name_async_annotations() {
        assert_eq!(
            parse_plain_name_string("[async]fetch").unwrap(),
            Some(PlainName::Async(
                Label::new("fetch".to_owned()),
                Label::new("fetch".to_owned())
            ))
        );
        assert_eq!(
            parse_plain_name_string("[async method]stream.read").unwrap(),
            Some(PlainName::AsyncMethod(
                Label::new("stream".to_owned()),
                Label::new("read".to_owned())
            ))
        );
        assert_eq!(
            parse_plain_name_string("[async static]stream.new").unwrap(),
            Some(PlainName::AsyncStatic(
                Label::new("stream".to_owned()),
                Label::new("new".to_owned())
            ))
        );
    }

    #[test]
    fn test_export_name_interface() -> anyhow::Result<()> {
        let name = "foo:bar/baz@1.0.0";
        let export_name = parse_interface_name_string(name).unwrap();
        assert_eq!(
            export_name,
            Some(InterfaceName {
                namespace: "foo".to_string(),
                label: Label::new("bar".to_string()),
                projection: "baz".to_string(),
                version: Some(Version::parse("1.0.0").unwrap()),
            })
        );
        Ok(())
    }

    #[test]
    fn test_nested_interface_name() {
        let name = "a:b-c:d-e:f-g/h-i/j-k/l-m/n/o/p@1.0.0";
        let export_name = parse_interface_name_string(name).unwrap().unwrap();
        assert_eq!(export_name.namespace, "a:b-c:d-e");
        assert_eq!(export_name.label, Label::new("f-g"));
        assert_eq!(export_name.projection, "h-i/j-k/l-m/n/o/p");
        assert_eq!(export_name.version, Some(Version::parse("1.0.0").unwrap()));
    }

    #[test]
    fn test_nested_package_path() {
        let (path, trailing) = parse_package_path("a:b:c:d/e/f/g@1.2.3").unwrap();
        assert_eq!(path.namespace, "a:b:c");
        assert_eq!(path.name, "d/e/f/g");
        assert_eq!(trailing, "@1.2.3");
    }
}
