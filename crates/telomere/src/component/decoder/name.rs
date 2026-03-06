use crate::binary::BinaryReader;
use crate::component::decoder::{ComponentParseError, ParseContext, ParseResult};
use crate::component::ir::{
    Dependency, ExportName, HashName, ImportName, InterfaceName, Label, LockedDependency,
    PackagePath, ParsedExportName, ParsedImportName, PlainName, UnlockedDependency, UrlName,
    VersionRange,
};
use crate::parser::core::parse_name;
use once_cell::sync::Lazy;
use regex::Regex;
use semver::Version;
use tracing::trace;

static LABEL: Lazy<Regex> =
    Lazy::new(|| Regex::new("^(?:[A-Za-z][A-Za-z0-9]*)(?:-[A-Za-z0-9]+)*$").unwrap());
static WORDS: Lazy<Regex> =
    Lazy::new(|| Regex::new("^[a-z][0-9a-z]*(?:-[a-z][0-9a-z]*)*$").unwrap());
static INTERFACE_NAME: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^(?<namespace>[a-z][0-9a-z-]*):(?<label>[a-zA-Z][a-zA-Z0-9-]*)/(?<projection>[a-zA-Z][a-zA-Z0-9-]*)(|@(?<version>[0-9A-Za-z.+<>=\-]+))$").unwrap()
});

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
    let plain_name = parse_plain_name_string(name.as_str())?;
    if let Some(parsed) = plain_name {
        return Ok(ExportName {
            original: name,
            parsed: ParsedExportName::Plain(parsed),
        });
    }
    if let Some(parsed) =
        parse_interface_name(name.as_str()).map_err(ComponentParseError::InvalidExportName)?
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

fn parse_plain_name_string(text: &str) -> ParseResult<Option<PlainName>> {
    trace!("parse_plain_name_string: {}", text);
    match text.as_bytes() {
        #[cfg(feature = "component-gated-feature-async")]
        // async
        [b'[', b'a', b's', b'y', b'n', b'c', b']', ..] => Err(ComponentParseError::Unsupported(
            "async plain names are not supported".to_owned(),
        )),
        // constructor
        [b'[', b'c', b'o', b'n', b's', b't', b'r', b'u', b'c', b't', b'o', b'r', b']', ..] => {
            Ok(Some(PlainName::Constructor(
                parse_label(&text[13..]).map_err(ComponentParseError::InvalidExportName)?,
            )))
        }
        // method
        [b'[', b'm', b'e', b't', b'h', b'o', b'd', b']', ..] => {
            match *text[8..].split(".").collect::<Vec<_>>() {
                [a, b] => {
                    let a = parse_label(a).map_err(ComponentParseError::InvalidExportName)?;
                    let b = parse_label(b).map_err(ComponentParseError::InvalidExportName)?;
                    Ok(Some(PlainName::Method(a, b)))
                }
                _ => {
                    // invalid static
                    Err(ComponentParseError::InvalidExportName(format!(
                        "Invalid method export name: {}",
                        text
                    )))
                }
            }
        }
        #[cfg(feature = "component-gated-feature-async")]
        // async method
        [b'[', b'a', b's', b'y', b'n', b'c', b' ', b'm', b'e', b't', b'h', b'o', b'd', b']', ..] => {
            Err(ComponentParseError::Unsupported(
                "async method names are not supported".to_owned(),
            ))
        }
        // static
        [b'[', b's', b't', b'a', b't', b'i', b'c', b']', ..] => {
            match *text[8..].split(".").collect::<Vec<_>>() {
                [a, b] => {
                    let a = parse_label(a).map_err(ComponentParseError::InvalidExportName)?;
                    let b = parse_label(b).map_err(ComponentParseError::InvalidExportName)?;
                    Ok(Some(PlainName::Static(a, b)))
                }
                _ => {
                    // invalid static
                    Err(ComponentParseError::InvalidExportName(format!(
                        "Invalid static export name: {}",
                        text
                    )))
                }
            }
        }
        #[cfg(feature = "component-gated-feature-async")]
        // async static
        [b'[', b'a', b's', b'y', b'n', b'c', b' ', b's', b't', b'a', b't', b'i', b'c', b']', ..] => {
            Err(ComponentParseError::Unsupported(
                "async static names are not supported".to_owned(),
            ))
        }
        _ if LABEL.is_match(text) => Ok(Some(PlainName::Plain(Label::new(text)))),
        _ => Ok(None),
    }
}

fn parse_dep_name_string(text: &str) -> Result<Option<Dependency>, String> {
    if let Some(rest) = text.strip_prefix("unlocked-dep=") {
        return Ok(Some(Dependency::Unlocked(parse_unlocked_dependency(rest)?)));
    }
    if let Some(rest) = text.strip_prefix("locked-dep=") {
        return Ok(Some(Dependency::Locked(parse_locked_dependency(rest)?)));
    }
    Ok(None)
}

fn parse_url_name_string(text: &str) -> Result<Option<UrlName>, String> {
    let Some(rest) = text.strip_prefix("url=") else {
        return Ok(None);
    };
    let rest = rest
        .strip_prefix('<')
        .ok_or_else(|| format!("expected `<` at `{rest}`"))?;
    let Some(end) = rest.find('>') else {
        return Err("failed to find `>`".to_owned());
    };
    let url = &rest[..end];
    if url.contains('<') {
        return Err("url cannot contain `<`".to_owned());
    }
    Ok(Some(UrlName {
        url: url.to_owned(),
        hash_name: parse_integrity_suffix(&rest[end + 1..])?,
    }))
}

fn parse_hash_name_string(text: &str) -> Result<Option<HashName>, String> {
    let Some(rest) = text.strip_prefix("integrity=") else {
        return Ok(None);
    };
    Ok(Some(HashName {
        integrity: parse_integrity_bracket(rest)?,
    }))
}

fn parse_version_range(text: &str) -> Result<VersionRange, String> {
    if text == "*" {
        Ok(VersionRange::Any)
    } else {
        let inner = text
            .strip_prefix('{')
            .and_then(|v| v.strip_suffix('}'))
            .ok_or_else(|| "expected `>=` or `<` at start of version range".to_owned())?;
        if let Some(version) = inner.strip_prefix(">=") {
            if let Some((lower, upper)) = version.split_once(" <") {
                Ok(VersionRange::Ranged {
                    lower: Some(parse_semver(lower)?),
                    upper: Some(parse_semver(upper)?),
                })
            } else {
                Ok(VersionRange::Ranged {
                    lower: Some(parse_semver(version)?),
                    upper: None,
                })
            }
        } else if let Some(version) = inner.strip_prefix('<') {
            Ok(VersionRange::Ranged {
                lower: None,
                upper: Some(parse_semver(version)?),
            })
        } else {
            Err("expected `>=` or `<` at start of version range".to_owned())
        }
    }
}

fn parse_label(text: &str) -> Result<Label, String> {
    if LABEL.is_match(text) {
        // valid label
        Ok(Label::new(text.to_string()))
    } else {
        Err(format!("Invalid label: {}", text))
    }
}

pub(crate) fn is_kebab_label(text: &str) -> bool {
    let mut segments = text.split('-');
    let Some(first) = segments.next() else {
        return false;
    };
    if !is_kebab_segment(first, false) {
        return false;
    }
    for segment in segments {
        if !is_kebab_segment(segment, true) {
            return false;
        }
    }
    true
}

fn is_kebab_segment(segment: &str, allow_numeric_only: bool) -> bool {
    if segment.is_empty() || !segment.chars().all(|ch| ch.is_ascii_alphanumeric()) {
        return false;
    }
    let mut has_lower = false;
    let mut has_upper = false;
    for ch in segment.chars() {
        has_lower |= ch.is_ascii_lowercase();
        has_upper |= ch.is_ascii_uppercase();
    }
    match (has_lower, has_upper) {
        (true, true) => false,
        (false, false) => allow_numeric_only,
        _ => true,
    }
}

fn parse_interface_name_string(text: &str) -> Result<Option<InterfaceName>, String> {
    if !text.contains(':') && !text.contains('/') {
        return Ok(None);
    }
    let Some((namespace, after_namespace)) = text.split_once(':') else {
        return Ok(None);
    };
    let namespace = parse_words(namespace)?;
    let (path, version) = match after_namespace.split_once('@') {
        Some((_path, "")) => return Err("empty string".to_owned()),
        Some((path, version)) => (path, Some(parse_semver(version)?)),
        None => (after_namespace, None),
    };
    let Some((label, projection)) = path.split_once('/') else {
        return Err("expected `/` after package name".to_owned());
    };
    if let Some((_, trailing)) = projection.split_once('/') {
        return Err(format!("trailing characters found: `/{trailing}`"));
    }
    Ok(Some(InterfaceName {
        namespace,
        label: Label::new(parse_words(label)?),
        projection: Label::new(parse_words(projection)?),
        version,
    }))
}

fn parse_interface_name(text: &str) -> Result<Option<InterfaceName>, String> {
    let captures = INTERFACE_NAME.captures(text);
    if let Some(captures) = captures {
        let namespace = parse_words(captures.name("namespace").unwrap().as_str())?;
        let label = Label::new(parse_words(captures.name("label").unwrap().as_str())?);
        let projection = Label::new(parse_words(captures.name("projection").unwrap().as_str())?);
        let version = captures
            .name("version")
            .map(|v| Version::parse(v.as_str()))
            .map_or(Ok(None), |v| v.map(Some))
            .map_err(|x| format!("semver error: {}", x))?;
        Ok(Some(InterfaceName {
            namespace,
            label,
            projection,
            version,
        }))
    } else {
        Ok(None)
    }
}

fn parse_words(text: &str) -> Result<String, String> {
    if WORDS.is_match(text) {
        // valid words
        Ok(text.to_string())
    } else {
        Err(format!("Invalid words: {}", text))
    }
}

fn parse_unlocked_dependency(rest: &str) -> Result<UnlockedDependency, String> {
    let rest = rest
        .strip_prefix('<')
        .ok_or_else(|| format!("expected `<` at `{rest}`"))?;
    let (package, rest) = parse_package_path(rest)?;
    let version_range = match rest.chars().next() {
        Some('>') => {
            if rest != ">" {
                return Err(format!("trailing characters found: `{}`", &rest[1..]));
            }
            None
        }
        Some('@') => {
            let body = &rest[1..];
            if let Some(body) = body.strip_prefix('*') {
                if body != ">" {
                    return Err(format!("trailing characters found: `{body}`"));
                }
                Some(VersionRange::Any)
            } else {
                let Some(body) = body.strip_prefix('{') else {
                    let at = body
                        .chars()
                        .next()
                        .map(|c| c.to_string())
                        .unwrap_or_default();
                    return Err(format!("expected `{{` at `{at}`"));
                };
                let Some(end) = body.find('}') else {
                    return Err("expected `}` in version range".to_owned());
                };
                let range = format!("{{{}}}", &body[..end]);
                let trailing = &body[end + 1..];
                if trailing != ">" {
                    return Err(format!("trailing characters found: `{trailing}`"));
                }
                Some(parse_version_range(&range)?)
            }
        }
        Some(other) => return Err(format!("expected `>` or `@` at `{other}`")),
        None => return Err("expected `>` at ``".to_owned()),
    };
    Ok(UnlockedDependency {
        package,
        version_range,
    })
}

fn parse_locked_dependency(rest: &str) -> Result<LockedDependency, String> {
    let rest = rest
        .strip_prefix('<')
        .ok_or_else(|| format!("expected `<` at `{rest}`"))?;
    let (package, rest) = parse_package_path(rest)?;
    let (version, suffix) = match rest.chars().next() {
        Some('>') => (None, &rest[1..]),
        Some('@') => {
            let Some(end) = rest[1..].find('>') else {
                return Err("expected `>` at ``".to_owned());
            };
            let version = parse_semver(&rest[1..][..end])?;
            (Some(version), &rest[1..][end + 1..])
        }
        Some(other) => return Err(format!("expected `>` at `{other}`")),
        None => return Err("expected `>` at ``".to_owned()),
    };
    Ok(LockedDependency {
        package,
        version,
        hash_name: parse_integrity_suffix(suffix)?,
    })
}

fn parse_package_path(rest: &str) -> Result<(PackagePath, &str), String> {
    let Some((namespace, rest)) = rest.split_once(':') else {
        return Err("expected `/` after package name".to_owned());
    };
    let namespace = parse_words(namespace)?;
    let end = rest.find(['@', '>']).unwrap_or(rest.len());
    let name = &rest[..end];
    let trailing = &rest[end..];
    let name = parse_words(name)?;
    Ok((PackagePath { namespace, name }, trailing))
}

fn parse_semver(text: &str) -> Result<Version, String> {
    Version::parse(text).map_err(|_| format!("`{text}` is not a valid semver"))
}

fn parse_integrity_suffix(suffix: &str) -> Result<Option<HashName>, String> {
    if suffix.is_empty() {
        return Ok(None);
    }
    let Some(rest) = suffix.strip_prefix(",integrity=") else {
        if suffix == "," {
            return Err("expected `integrity=<`".to_owned());
        }
        return Err(format!("trailing characters found: `{suffix}`"));
    };
    Ok(Some(HashName {
        integrity: parse_integrity_bracket(rest)?,
    }))
}

fn parse_integrity_bracket(rest: &str) -> Result<String, String> {
    let rest = rest
        .strip_prefix('<')
        .ok_or_else(|| format!("expected `<` at `{rest}`"))?;
    let Some(end) = rest.find('>') else {
        return Err("failed to find `>`".to_owned());
    };
    let integrity = &rest[..end];
    validate_integrity(integrity)?;
    let trailing = &rest[end + 1..];
    if !trailing.is_empty() {
        return Err(format!("trailing characters found: `{trailing}`"));
    }
    Ok(integrity.to_owned())
}

fn validate_integrity(integrity: &str) -> Result<(), String> {
    let integrity = integrity.trim();
    if integrity.is_empty() {
        return Err("integrity hash cannot be empty".to_owned());
    }
    for entry in integrity.split_whitespace() {
        let Some((algorithm, digest)) = entry.split_once('-') else {
            return Err("expected `-` after hash algorithm".to_owned());
        };
        match algorithm {
            "sha256" | "sha384" | "sha512" => {}
            _ => return Err("unrecognized hash algorithm".to_owned()),
        }
        if !is_valid_integrity_digest(digest) {
            return Err("not valid base64".to_owned());
        }
    }
    Ok(())
}

fn is_valid_integrity_digest(digest: &str) -> bool {
    if digest.is_empty() || digest.bytes().all(|b| b == b'=') {
        return false;
    }
    let mut seen_padding = false;
    let mut padding = 0usize;
    for ch in digest.chars() {
        if !(ch.is_ascii_alphanumeric() || matches!(ch, '+' | '/' | '?' | '=' | '.' | '_' | '-')) {
            return false;
        }
        if ch == '=' {
            seen_padding = true;
            padding += 1;
            if padding > 2 {
                return false;
            }
        } else if seen_padding {
            return false;
        }
    }
    !digest.starts_with('=')
}

#[cfg(test)]
mod tests {
    use super::{parse_interface_name, parse_plain_name_string};
    use crate::component::ir::{InterfaceName, Label, PlainName};
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
        let export_name = parse_interface_name(name).unwrap();
        assert_eq!(
            export_name,
            Some(InterfaceName {
                namespace: "foo".to_string(),
                label: Label::new("bar".to_string()),
                projection: Label::new("baz".to_string()),
                version: Some(Version::parse("1.0.0").unwrap()),
            })
        );
        Ok(())
    }
}
