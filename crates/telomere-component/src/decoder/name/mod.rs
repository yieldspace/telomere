mod dependency;
mod interface;
mod plain;

use crate::decoder::{ComponentParseError, ParseContext, ParseResult};
use crate::ir::{
    ExportName, ExternNameAttributes, ImportName, InterfaceName, Label, ParsedExportName,
    ParsedImportName, PlainName,
};
use crate::support::binary::BinaryReader;
use crate::support::parser::core::{parse_name, parse_u32};
use tracing::trace;

use dependency::{parse_dep_name_string, parse_hash_name_string, parse_url_name_string};
use interface::{build_interface_name, parse_interface_name_parts, InterfaceNameParts};
pub(crate) use plain::is_kebab_label;
use plain::{parse_label, parse_plain_name_string};

/// The name options of the split extern-name encoding (name prefix
/// 0x02), as raw strings, before validation.
#[derive(Default)]
struct NameOptions {
    implements: Option<String>,
    version_suffix: Option<String>,
    external_id: Option<String>,
}

/// Which side of an extern name is being decoded; selects the error
/// variant used for name errors.
#[derive(Clone, Copy)]
enum NameSide {
    Import,
    Export,
}

impl NameSide {
    fn invalid(self, message: String) -> ComponentParseError {
        match self {
            NameSide::Import => ComponentParseError::InvalidImportName(message),
            NameSide::Export => ComponentParseError::InvalidExportName(message),
        }
    }

    fn desc(self) -> &'static str {
        match self {
            NameSide::Import => "import",
            NameSide::Export => "export",
        }
    }
}

/// The base name, classified for name-option validation.
enum BaseKind {
    /// A plain name; `true` when it is a bare label, the only shape an
    /// `implements` option may accompany.
    PlainBare(bool),
    /// A dependency, URL or hash import name.
    Other,
    /// An interface name, not yet version-resolved.
    Interface(InterfaceNameParts),
}

/// The outcome of validating the name options against the base name.
struct ValidatedOptions {
    attributes: ExternNameAttributes,
    /// The base interface name with its version resolved against the
    /// `version-suffix`, when the base name is an interface name.
    interface: Option<InterfaceName>,
}

/// Validates the split-encoding name options against the base name,
/// mirroring the name-only checks of wasmparser's `validate_extern`
/// (the checks that depend on the entity sort run at the call sites via
/// [`validate_name_attributes_sort`], once the sort is known).
fn validate_name_options(
    side: NameSide,
    raw: &str,
    base: BaseKind,
    options: &NameOptions,
) -> Result<ValidatedOptions, String> {
    if let Some(suffix) = &options.version_suffix {
        if suffix.is_empty() {
            return Err("version suffix cannot be empty".to_owned());
        }
        // Without `implements` the suffix applies to the name itself,
        // so the name must be an interface name.
        if options.implements.is_none() && !matches!(base, BaseKind::Interface(_)) {
            return Err("`version-suffix` requires an interface name or `implements`".to_owned());
        }
    }

    let mut implements = None;
    if let Some(value) = &options.implements {
        if !matches!(base, BaseKind::PlainBare(true)) {
            return Err(format!("name `{raw}` is not valid with `implements`"));
        }
        implements = Some(parse_implements(value, options.version_suffix.as_deref())?);
    }

    let interface = match base {
        BaseKind::Interface(parts) => Some(
            build_interface_name(parts, options.version_suffix.as_deref()).map_err(|detail| {
                format!(
                    "{} name `{raw}` is not valid: invalid interface version: {detail}",
                    side.desc()
                )
            })?,
        ),
        _ => None,
    };

    Ok(ValidatedOptions {
        attributes: ExternNameAttributes {
            implements,
            version_suffix: options.version_suffix.clone(),
            external_id: options.external_id.clone(),
        },
        interface,
    })
}

/// Parses an `implements` option value, which must be an interface
/// name; its version is resolved against the `version-suffix`.
fn parse_implements(value: &str, version_suffix: Option<&str>) -> Result<InterfaceName, String> {
    let parts = match parse_interface_name_parts(value) {
        Ok(parts) => parts,
        Err(_) => return Err(format!("`{value}` is not a valid name")),
    };
    if let Some(parts) = parts {
        return build_interface_name(parts, version_suffix)
            .map_err(|detail| format!("invalid interface version: {detail}"));
    }
    if parse_plain_name_string(value).ok().flatten().is_some() {
        return Err(format!("name `{value}` must be an interface"));
    }
    Err(format!("`{value}` is not a valid name"))
}

/// Checks the name options that depend on the entity sort: `implements`
/// and `version-suffix` may only accompany instances.
pub(crate) fn validate_name_attributes_sort(
    is_instance: bool,
    attributes: &ExternNameAttributes,
) -> Result<(), String> {
    if attributes.implements.is_some() && !is_instance {
        return Err("only instances can have an `implements`".to_owned());
    }
    if attributes.version_suffix.is_some() && !is_instance {
        return Err("only instances can have a `version-suffix`".to_owned());
    }
    Ok(())
}

/// Reads the option vector of a split extern name (the part after the
/// base name when the name prefix is 0x02).
fn parse_name_options(
    ctx: &mut ParseContext<impl BinaryReader>,
    side: NameSide,
) -> ParseResult<NameOptions> {
    let (_, count) = parse_u32(ctx.reader)?;
    let mut options = NameOptions::default();
    for _ in 0..count {
        let tag = ctx.reader.read_exact_one()?;
        let (_, value) = parse_name(ctx.reader)?;
        let (slot, duplicate) = match tag {
            0x00 => (&mut options.implements, "duplicate `implements` attribute"),
            0x01 => (
                &mut options.version_suffix,
                "duplicate `version-suffix` attribute",
            ),
            0x02 => (
                &mut options.external_id,
                "duplicate `external-id` attribute",
            ),
            _ => {
                return Err(side.invalid(format!("invalid extern name option: {tag:#04x}")));
            }
        };
        if slot.is_some() {
            return Err(side.invalid(duplicate.to_owned()));
        }
        *slot = Some(value);
    }
    Ok(options)
}

/// Reads the leading byte(s) of an extern name: `0x00`/`0x01` introduce
/// a plain name, `0x02` a name followed by an option vector.
fn parse_name_head(
    ctx: &mut ParseContext<impl BinaryReader>,
    side: NameSide,
) -> ParseResult<(String, NameOptions)> {
    let split = match ctx.reader.read_exact_one()? {
        0x00 | 0x01 => false,
        0x02 => true,
        magic => {
            ComponentParseError::assert_magic([magic], [0x00], &format!("{} name", side.desc()))?;
            unreachable!("assert_magic only returns Ok for the expected magic")
        }
    };
    let (_, name) = parse_name(ctx.reader)?;
    let options = if split {
        parse_name_options(ctx, side)?
    } else {
        NameOptions::default()
    };
    Ok((name, options))
}

pub fn parse_import_name_dash(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> ParseResult<ImportName> {
    let (name, options) = parse_name_head(ctx, NameSide::Import)?;
    build_import_name(name, &options)
}

pub fn parse_export_name_dash(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> ParseResult<ExportName> {
    trace!("parse_export_name_dash");
    let (name, options) = parse_name_head(ctx, NameSide::Export)?;
    build_export_name(name, &options)
}

pub fn parse_export_name(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<ExportName> {
    trace!("parse_export_name");
    let (_, name) = parse_name(ctx.reader)?;
    build_export_name(name, &NameOptions::default())
}

fn build_export_name(name: String, options: &NameOptions) -> ParseResult<ExportName> {
    if name.starts_with("relative-url=") {
        return Err(ComponentParseError::InvalidExportName(format!(
            "`{name}` is not a valid extern name"
        )));
    }
    if let Some(parsed) = parse_plain_name_string(name.as_str())? {
        let validated = validate_name_options(
            NameSide::Export,
            &name,
            BaseKind::PlainBare(matches!(parsed, PlainName::Plain(_))),
            options,
        )
        .map_err(ComponentParseError::InvalidExportName)?;
        return Ok(ExportName {
            original: name,
            parsed: ParsedExportName::Plain(parsed),
            attributes: validated.attributes,
        });
    }
    if let Some(parts) =
        parse_interface_name_parts(name.as_str()).map_err(ComponentParseError::InvalidExportName)?
    {
        let validated =
            validate_name_options(NameSide::Export, &name, BaseKind::Interface(parts), options)
                .map_err(ComponentParseError::InvalidExportName)?;
        return Ok(ExportName {
            original: name,
            parsed: ParsedExportName::Interface(validated.interface.unwrap()),
            attributes: validated.attributes,
        });
    }
    // Names with options that match no name production still owe the
    // option checks (e.g. `implements` on an unparseable name).
    validate_name_options(NameSide::Export, &name, BaseKind::Other, options)
        .map_err(ComponentParseError::InvalidExportName)?;
    Err(ComponentParseError::InvalidExportName(format!(
        "`{}` is not a valid extern name",
        name
    )))
}

fn build_import_name(name: String, options: &NameOptions) -> ParseResult<ImportName> {
    if name.starts_with("relative-url=") {
        return Err(ComponentParseError::InvalidImportName(format!(
            "`{name}` is not a valid extern name"
        )));
    }
    if let Some(parsed) = parse_plain_name_string(name.as_str())? {
        let validated = validate_name_options(
            NameSide::Import,
            &name,
            BaseKind::PlainBare(matches!(parsed, PlainName::Plain(_))),
            options,
        )
        .map_err(ComponentParseError::InvalidImportName)?;
        return Ok(ImportName {
            original: name,
            parsed: ParsedImportName::Plain(parsed),
            attributes: validated.attributes,
        });
    }
    if let Some(parsed) =
        parse_dep_name_string(name.as_str()).map_err(ComponentParseError::InvalidImportName)?
    {
        let validated = validate_name_options(NameSide::Import, &name, BaseKind::Other, options)
            .map_err(ComponentParseError::InvalidImportName)?;
        return Ok(ImportName {
            original: name,
            parsed: ParsedImportName::Dependency(parsed),
            attributes: validated.attributes,
        });
    }
    if let Some(parsed) =
        parse_url_name_string(name.as_str()).map_err(ComponentParseError::InvalidImportName)?
    {
        let validated = validate_name_options(NameSide::Import, &name, BaseKind::Other, options)
            .map_err(ComponentParseError::InvalidImportName)?;
        return Ok(ImportName {
            original: name,
            parsed: ParsedImportName::Url(parsed),
            attributes: validated.attributes,
        });
    }
    if let Some(parsed) =
        parse_hash_name_string(name.as_str()).map_err(ComponentParseError::InvalidImportName)?
    {
        let validated = validate_name_options(NameSide::Import, &name, BaseKind::Other, options)
            .map_err(ComponentParseError::InvalidImportName)?;
        return Ok(ImportName {
            original: name,
            parsed: ParsedImportName::Hash(parsed),
            attributes: validated.attributes,
        });
    }
    if let Some(parts) =
        parse_interface_name_parts(name.as_str()).map_err(ComponentParseError::InvalidImportName)?
    {
        let validated =
            validate_name_options(NameSide::Import, &name, BaseKind::Interface(parts), options)
                .map_err(ComponentParseError::InvalidImportName)?;
        return Ok(ImportName {
            original: name,
            parsed: ParsedImportName::Interface(validated.interface.unwrap()),
            attributes: validated.attributes,
        });
    }
    validate_name_options(NameSide::Import, &name, BaseKind::Other, options)
        .map_err(ComponentParseError::InvalidImportName)?;
    Err(ComponentParseError::InvalidImportName(format!(
        "`{}` is not a valid extern name",
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
    use super::interface::{parse_interface_name_string, split_canonical_version};
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
    fn test_nested_interface_name_rejected() {
        // Nested namespaces and nested projections are gated future
        // syntax; the package-path grammar admits exactly one segment
        // each for namespace, package and projection.
        let name = "a:b-c:d-e:f-g/h-i/j-k/l-m/n/o/p@1.0.0";
        assert_eq!(
            parse_interface_name_string(name).unwrap_err(),
            "expected `/` after package name"
        );
        let name = "a:b/c/d@1.0.0";
        assert_eq!(
            parse_interface_name_string(name).unwrap_err(),
            "trailing characters found: `/d`"
        );
    }

    #[test]
    fn test_interface_name_version_must_be_semver_without_suffix() {
        assert_eq!(
            parse_interface_name_string("a:b/c@1").unwrap_err(),
            "`1` is not a valid semver"
        );
        assert_eq!(
            parse_interface_name_string("a:b/c@0.2").unwrap_err(),
            "`0.2` is not a valid semver"
        );
        assert!(parse_interface_name_string("a:b/c@0.2.0")
            .unwrap()
            .is_some());
    }

    #[test]
    fn test_split_canonical_version() {
        assert_eq!(split_canonical_version("1.2.3"), Some(("1", Some(".2.3"))));
        assert_eq!(split_canonical_version("0.2.6"), Some(("0.2", Some(".6"))));
        assert_eq!(split_canonical_version("0.0.1"), Some(("0.0.1", None)));
        assert_eq!(
            split_canonical_version("0.0.1+sha"),
            Some(("0.0.1", Some("+sha")))
        );
        assert_eq!(
            split_canonical_version("1.2.3-alpha.1+sha"),
            Some(("1.2.3-alpha.1", Some("+sha")))
        );
        assert_eq!(split_canonical_version("1.2"), None);
        assert_eq!(split_canonical_version("1.02.3"), None);
        assert_eq!(split_canonical_version("1.2.3+"), None);
    }

    #[test]
    fn test_nested_package_path() {
        let (path, trailing) = parse_package_path("a:b:c:d/e/f/g@1.2.3").unwrap();
        assert_eq!(path.namespace, "a:b:c");
        assert_eq!(path.name, "d/e/f/g");
        assert_eq!(trailing, "@1.2.3");
    }
}
