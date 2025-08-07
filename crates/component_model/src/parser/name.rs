use crate::name::{
    Dependency, ExportName, HashName, ImportName, InterfaceName, Label, LockedDependency,
    PackagePath, ParsedExportName, ParsedImportName, PlainName, UnlockedDependency, UrlName,
    VersionRange,
};
use crate::Result;
use crate::{ComponentParseError, ComponentParser};
use binary_reader::BinaryReader;
use once_cell::sync::Lazy;
use regex::Regex;
use semver::Version;
use telomere_wasm::parser::core::parse_name;
use tracing::trace;

static LABEL: Lazy<Regex> = Lazy::new(|| {
    Regex::new("^(?:[a-z][0-9a-z]*|[A-Z][0-9A-Z]*)(?:-(?:[a-z][0-9a-z]*|[A-Z][0-9A-Z]*))*$")
        .unwrap()
});
static WORDS: Lazy<Regex> =
    Lazy::new(|| Regex::new("^[a-z][0-9a-z]*(?:-[a-z][0-9a-z]*)*$").unwrap());
static INTERFACE_NAME: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^(?<namespace>[a-z][0-9a-z-]*):(?<label>[a-zA-Z][a-zA-Z0-9-]*)/(?<projection>[a-zA-Z][a-zA-Z0-9-]*)(|@(?<version>[0-9.><=\-]+))$").unwrap()
});

impl<T> ComponentParser<'_, T>
where
    T: BinaryReader,
{
    pub fn parse_import_name_dash(&mut self) -> Result<ImportName> {
        match self.reader.read_exact_one()? {
            0x00 => self.parse_import_name(),
            x => Err(ComponentParseError::InvalidSignature(
                Box::new([x]),
                Box::new([0x00]),
                "import name magic".to_string(),
            )),
        }
    }

    pub fn parse_export_name_dash(&mut self) -> Result<ExportName> {
        match self.reader.read_exact_one()? {
            0x00 => self.parse_export_name(),
            x => Err(ComponentParseError::InvalidSignature(
                Box::new([x]),
                Box::new([0x00]),
                "export name magic".to_string(),
            )),
        }
    }

    pub fn parse_import_name(&mut self) -> Result<ImportName> {
        let (_, name) = parse_name(self.reader)?;
        if let Some(parsed) = parse_plain_name_string(name.as_str())? {
            return Ok(ImportName {
                original: name,
                parsed: ParsedImportName::Plain(parsed),
            });
        }
        if let Some(parsed) = parse_interface_name(name.as_str())? {
            return Ok(ImportName {
                original: name,
                parsed: ParsedImportName::Interface(parsed),
            });
        }
        if let Some(parsed) = parse_dep_name_string(name.to_string())? {
            return Ok(ImportName {
                original: name,
                parsed: ParsedImportName::Dependency(parsed),
            });
        }
        if let Some(parsed) = parse_url_name_string(name.as_str())? {
            return Ok(ImportName {
                original: name,
                parsed: ParsedImportName::Url(parsed),
            });
        }
        if let Some(parsed) = parse_hash_name_string(name.as_str())? {
            return Ok(ImportName {
                original: name,
                parsed: ParsedImportName::Hash(parsed),
            });
        }
        Err(ComponentParseError::InvalidName(format!(
            "RawImport name: `{}`",
            name
        )))
    }

    pub fn parse_export_name(&mut self) -> Result<ExportName> {
        let (_, name) = parse_name(self.reader)?;
        let plain_name = parse_plain_name_string(name.as_str())?;
        if let Some(parsed) = plain_name {
            return Ok(ExportName {
                original: name,
                parsed: ParsedExportName::Plain(parsed),
            });
        }
        if let Some(parsed) = parse_interface_name(name.as_str())? {
            return Ok(ExportName {
                original: name,
                parsed: ParsedExportName::Interface(parsed),
            });
        }
        Err(ComponentParseError::InvalidName(format!(
            "Export name: `{}`",
            name
        )))
    }

    pub fn parse_label_dash(&mut self) -> Result<Label> {
        let (_, name) = parse_name(self.reader)?;
        parse_label(&name)
    }
}

fn parse_plain_name_string(text: &str) -> Result<Option<PlainName>> {
    trace!("parse_plain_name_string: {}", text);
    match text.as_bytes() {
        #[cfg(feature = "async")]
        // async
        [b'[', b'a', b's', b'y', b'n', b'c', b']', ..] => {
            todo!()
        }
        // constructor
        [b'[', b'c', b'o', b'n', b's', b't', b'r', b'u', b'c', b't', b'o', b'r', b']', ..] => {
            Ok(Some(PlainName::Constructor(parse_label(&text[13..])?)))
        }
        // method
        [b'[', b'm', b'e', b't', b'h', b'o', b'd', b']', ..] => {
            match *text[8..].split(".").collect::<Vec<_>>() {
                [a, b] => {
                    let a = parse_label(a)?;
                    let b = parse_label(b)?;
                    Ok(Some(PlainName::Method(a, b)))
                }
                _ => {
                    // invalid static
                    Err(ComponentParseError::InvalidName(format!(
                        "Invalid method export name: {}",
                        text
                    )))
                }
            }
        }
        #[cfg(feature = "async")]
        // async method
        [b'[', b'a', b's', b'y', b'n', b'c', b' ', b'm', b'e', b't', b'h', b'o', b'd', b']', ..] => {
        }
        // static
        [b'[', b's', b't', b'a', b't', b'i', b'c', b']', ..] => {
            match *text[8..].split(".").collect::<Vec<_>>() {
                [a, b] => {
                    let a = parse_label(a)?;
                    let b = parse_label(b)?;
                    Ok(Some(PlainName::Static(a, b)))
                }
                _ => {
                    // invalid static
                    Err(ComponentParseError::InvalidName(format!(
                        "Invalid static export name: {}",
                        text
                    )))
                }
            }
        }
        #[cfg(feature = "async")]
        // async static
        [b'[', b'a', b's', b'y', b'n', b'c', b' ', b's', b't', b'a', b't', b'i', b'c', b']', ..] => {
        }
        _ if LABEL.is_match(text) => Ok(Some(PlainName::Plain(Label::new(text)))),
        _ => Ok(None),
    }
}

fn parse_dep_name_string(text: String) -> Result<Option<Dependency>> {
    static UNLOCKED_DEP: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"^unlocked-dep=<(?P<namespace>[a-z][a-z0-9\-]*):(?P<name>[a-z][a-z0-9\-]*)(|@(?P<verrange>\*|\{[^}]+}))>$").unwrap()
    });
    static LOCKED_DEP: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"^locked-dep=<(?P<namespace>[a-z][a-z0-9\-]*):(?P<name>[a-z][a-z0-9\-]*)(|@(?P<version>[^>]+))>(|,integrity=<(?P<integrity>[^>]+)>)$").unwrap()
    });
    if let Some(captures) = UNLOCKED_DEP.captures(&text) {
        let namespace = captures.name("namespace").unwrap().as_str().to_string();
        let name = captures.name("name").unwrap().as_str().to_string();

        Ok(Some(Dependency::Unlocked(UnlockedDependency {
            package: PackagePath { namespace, name },
            version_range: captures
                .name("verrange")
                .map(|x| parse_version_range(x.as_str()))
                .map_or(Ok(None), |x| x.map(Some))?,
        })))
    } else if let Some(captures) = LOCKED_DEP.captures(&text) {
        let namespace = captures.name("namespace").unwrap().as_str().to_string();
        let name = captures.name("name").unwrap().as_str().to_string();
        let version = if let Some(v) = captures.name("version") {
            Some(
                Version::parse(v.as_str())
                    .map_err(|x| x.to_string())
                    .map_err(ComponentParseError::InvalidName)?,
            )
        } else {
            None
        };
        let integrity = captures.name("integrity").map(|x| HashName {
            integrity: x.as_str().to_string(),
        });
        Ok(Some(Dependency::Locked(LockedDependency {
            package: PackagePath { namespace, name },
            version,
            hash_name: integrity,
        })))
    } else {
        Ok(None)
    }
}

fn parse_url_name_string(text: &str) -> Result<Option<UrlName>> {
    static URL_NAME: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"^url=<(?P<url>[^<>]*)>(|,integrity=<(?P<integrity>[^>]+)>)$").unwrap()
    });
    if let Some(captures) = URL_NAME.captures(text) {
        let url = captures.name("url").unwrap().as_str().to_string();
        Ok(Some(UrlName {
            url,
            hash_name: captures.name("integrity").map(|x| HashName {
                integrity: x.as_str().to_string(),
            }),
        }))
    } else {
        Ok(None)
    }
}

fn parse_hash_name_string(text: &str) -> Result<Option<HashName>> {
    static URL_NAME: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"^integrity=<(?P<integrity>[^>]+)>$").unwrap());
    if let Some(captures) = URL_NAME.captures(text) {
        let integrity = captures.name("integrity").unwrap().as_str().to_string();
        Ok(Some(HashName { integrity }))
    } else {
        Ok(None)
    }
}

fn parse_version_range(text: &str) -> Result<VersionRange> {
    static VER_LOWER: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"^\{>=(?P<version>[0-9.]+)}$").unwrap());
    static VER_UPPER: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"^\{<(?P<version>[0-9.]+)}$").unwrap());
    static VER_RANGE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"^\{>=(?P<lower>[0-9.]+) <(?P<upper>[0-9.]+)}$").unwrap());
    if text == "*" {
        Ok(VersionRange::Any)
    } else if let Some(captures) = VER_LOWER.captures(text) {
        let version = captures.name("version").unwrap().as_str();
        Ok(VersionRange::Ranged {
            lower: Some(
                Version::parse(version)
                    .map_err(|x| x.to_string())
                    .map_err(ComponentParseError::InvalidName)?,
            ),
            upper: None,
        })
    } else if let Some(captures) = VER_UPPER.captures(text) {
        let version = captures.name("version").unwrap().as_str();
        Ok(VersionRange::Ranged {
            upper: Some(
                Version::parse(version)
                    .map_err(|x| x.to_string())
                    .map_err(ComponentParseError::InvalidName)?,
            ),
            lower: None,
        })
    } else {
        let ranged = VER_RANGE
            .captures(text)
            .ok_or(format!("Invalid version: {}", text))
            .map_err(ComponentParseError::InvalidName)?;
        let lower = ranged.name("lower").unwrap().as_str();
        let upper = ranged.name("upper").unwrap().as_str();
        Ok(VersionRange::Ranged {
            lower: Some(
                Version::parse(lower)
                    .map_err(|x| x.to_string())
                    .map_err(ComponentParseError::InvalidName)?,
            ),
            upper: Some(
                Version::parse(upper)
                    .map_err(|x| x.to_string())
                    .map_err(ComponentParseError::InvalidName)?,
            ),
        })
    }
}

fn parse_label(text: &str) -> Result<Label> {
    if LABEL.is_match(text) {
        // valid label
        Ok(Label::new(text.to_string()))
    } else {
        Err(ComponentParseError::InvalidName(format!(
            "Invalid label: {}",
            text
        )))
    }
}

fn parse_interface_name(text: &str) -> Result<Option<InterfaceName>> {
    let captures = INTERFACE_NAME.captures(text);
    if let Some(captures) = captures {
        let namespace = parse_words(captures.name("namespace").unwrap().as_str())?;
        let label = parse_label(captures.name("label").unwrap().as_str())?;
        let projection = parse_label(captures.name("projection").unwrap().as_str())?;
        let version = captures
            .name("version")
            .map(|v| Version::parse(v.as_str()))
            .map_or(Ok(None), |v| v.map(Some))
            .map_err(|x| ComponentParseError::InvalidName(format!("Semver error: {}", x)))?;
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

fn parse_words(text: &str) -> Result<String> {
    if WORDS.is_match(text) {
        // valid words
        Ok(text.to_string())
    } else {
        Err(ComponentParseError::InvalidName(format!(
            "Invalid words: {}",
            text
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_interface_name, parse_plain_name_string};
    use crate::name::{InterfaceName, Label, PlainName};
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
