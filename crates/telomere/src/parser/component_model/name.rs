use crate::binary::BinaryReader;
use crate::component_model::{ExportName, InterfaceName, Label, PlainName};
use crate::parser::component_model::{ComponentParseError, ParseContext, ParseResult, SizedResult};
use crate::parser::core::parse_name;
use once_cell::sync::Lazy;
use regex::Regex;
use semver::Version;
use tracing::trace;

static LABEL: Lazy<Regex> = Lazy::new(|| {
    Regex::new("(?:[a-z][0-9a-z]*|[A-Z][0-9A-Z]*)(?:-(?:[a-z][0-9a-z]*|[A-Z][0-9A-Z]*))*").unwrap()
});
static WORDS: Lazy<Regex> = Lazy::new(|| Regex::new("[a-z][0-9a-z]*(?:-[a-z][0-9a-z]*)*").unwrap());
static INTERFACE_NAME: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^(?<namespace>[0-9a-z-]+):(?<label>[a-zA-Z0-9-]+)/(?<projection>[a-zA-Z0-9-]+)(|@(?<version>[0-9.><=\-]))$").unwrap()
});

mod plainname {
    use once_cell::sync::Lazy;
    use regex::Regex;

    pub static ASYNC: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\[async\](?<name>.+)$").unwrap());
}

pub fn parse_import_name_dash(ctx: &mut ParseContext<impl BinaryReader>) -> SizedResult<String> {
    ComponentParseError::assert_magic([ctx.reader.read_exact_one()?], [0x00], "import name")?;
    // todo: check name
    let (len, name) = parse_name(ctx.reader)?;
    Ok((len + 1, name))
}

pub fn parse_export_name_dash(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> SizedResult<ExportName> {
    trace!("parse_export_name_dash");
    ComponentParseError::assert_magic([ctx.reader.read_exact_one()?], [0x00], "export name")?;
    let (len, name) = parse_name(ctx.reader)?;
    Ok((len + 1, parse_export_name_string(name)?))
}

pub fn parse_export_name(ctx: &mut ParseContext<impl BinaryReader>) -> SizedResult<ExportName> {
    trace!("parse_export_name");
    let (len, name) = parse_name(ctx.reader)?;
    Ok((len + 1, parse_export_name_string(name)?))
}

pub fn parse_label_dash(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<Label> {
    let (_, name) = parse_name(ctx.reader)?;
    parse_label(&name).map_err(ComponentParseError::InvalidLabel)
}

fn parse_export_name_string(text: String) -> ParseResult<ExportName> {
    trace!("parse_export_name_string: {}", text);
    match text.as_bytes() {
        #[cfg(feature = "component-gated-feature-async")]
        // async
        [b'[', b'a', b's', b'y', b'n', b'c', b']', ..] => {
            todo!()
        }
        // constructor
        [b'[', b'c', b'o', b'n', b's', b't', b'r', b'u', b'c', b't', b'o', b'r', b']', ..] => {
            Ok(ExportName::Plain(PlainName::Constructor(
                parse_label(&text.as_str()[13..])
                    .map_err(ComponentParseError::InvalidExportName)?,
            )))
        }
        // method
        [b'[', b'm', b'e', b't', b'h', b'o', b'd', b']', ..] => {
            match *text.as_str()[8..].split(".").collect::<Vec<_>>() {
                [a, b] => {
                    let a = parse_label(a).map_err(ComponentParseError::InvalidExportName)?;
                    let b = parse_label(b).map_err(ComponentParseError::InvalidExportName)?;
                    Ok(ExportName::Plain(PlainName::Method(a, b)))
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
        }
        // static
        [b'[', b's', b't', b'a', b't', b'i', b'c', b']', ..] => {
            match *text.as_str()[8..].split(".").collect::<Vec<_>>() {
                [a, b] => {
                    let a = parse_label(a).map_err(ComponentParseError::InvalidExportName)?;
                    let b = parse_label(b).map_err(ComponentParseError::InvalidExportName)?;
                    Ok(ExportName::Plain(PlainName::Static(a, b)))
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
        }
        _ => {
            if LABEL.is_match(&text) {
                // valid label
                Ok(ExportName::Plain(PlainName::Plain(Label::new(text))))
            } else {
                Ok(ExportName::Interface(
                    parse_interface_name(text).map_err(ComponentParseError::InvalidExportName)?,
                ))
            }
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

fn parse_interface_name(text: String) -> Result<InterfaceName, String> {
    let captures = INTERFACE_NAME.captures(&text);
    if let Some(captures) = captures {
        let namespace = parse_words(captures.name("namespace").unwrap().as_str())?;
        let label = parse_label(captures.name("label").unwrap().as_str())?;
        let projection = parse_label(captures.name("projection").unwrap().as_str())?;
        let version = captures
            .name("version")
            .map(|v| Version::parse(v.as_str()))
            .map_or(Ok(None), |v| v.map(Some))
            .map_err(|x| format!("semver error: {}", x))?;
        Ok(InterfaceName {
            namespace,
            label,
            projection,
            version,
        })
    } else {
        Err(format!("parsing interface name is failed: {}", text))
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
