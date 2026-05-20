use super::*;
use crate::ir::{Label, PlainName};

pub(super) fn parse_plain_name_string(text: &str) -> ParseResult<Option<PlainName>> {
    trace!("parse_plain_name_string: {}", text);
    match text.as_bytes() {
        #[cfg(feature = "component-gated-feature-async")]
        [b'[', b'a', b's', b'y', b'n', b'c', b']', ..] => {
            let label = parse_label(&text[7..]).map_err(ComponentParseError::InvalidExportName)?;
            Ok(Some(PlainName::Async(label.clone(), label)))
        }
        [b'[', b'c', b'o', b'n', b's', b't', b'r', b'u', b'c', b't', b'o', b'r', b']', ..] => {
            Ok(Some(PlainName::Constructor(
                parse_label(&text[13..]).map_err(ComponentParseError::InvalidExportName)?,
            )))
        }
        [b'[', b'm', b'e', b't', b'h', b'o', b'd', b']', ..] => {
            match *text[8..].split('.').collect::<Vec<_>>() {
                [a, b] => {
                    let a = parse_label(a).map_err(ComponentParseError::InvalidExportName)?;
                    let b = parse_label(b).map_err(ComponentParseError::InvalidExportName)?;
                    Ok(Some(PlainName::Method(a, b)))
                }
                _ => Err(ComponentParseError::InvalidExportName(format!(
                    "Invalid method export name: {}",
                    text
                ))),
            }
        }
        #[cfg(feature = "component-gated-feature-async")]
        [b'[', b'a', b's', b'y', b'n', b'c', b' ', b'm', b'e', b't', b'h', b'o', b'd', b']', ..] => {
            match *text[14..].split('.').collect::<Vec<_>>() {
                [a, b] => {
                    let a = parse_label(a).map_err(ComponentParseError::InvalidExportName)?;
                    let b = parse_label(b).map_err(ComponentParseError::InvalidExportName)?;
                    Ok(Some(PlainName::AsyncMethod(a, b)))
                }
                _ => Err(ComponentParseError::InvalidExportName(format!(
                    "Invalid async method export name: {}",
                    text
                ))),
            }
        }
        [b'[', b's', b't', b'a', b't', b'i', b'c', b']', ..] => {
            match *text[8..].split('.').collect::<Vec<_>>() {
                [a, b] => {
                    let a = parse_label(a).map_err(ComponentParseError::InvalidExportName)?;
                    let b = parse_label(b).map_err(ComponentParseError::InvalidExportName)?;
                    Ok(Some(PlainName::Static(a, b)))
                }
                _ => Err(ComponentParseError::InvalidExportName(format!(
                    "Invalid static export name: {}",
                    text
                ))),
            }
        }
        #[cfg(feature = "component-gated-feature-async")]
        [b'[', b'a', b's', b'y', b'n', b'c', b' ', b's', b't', b'a', b't', b'i', b'c', b']', ..] => {
            match *text[14..].split('.').collect::<Vec<_>>() {
                [a, b] => {
                    let a = parse_label(a).map_err(ComponentParseError::InvalidExportName)?;
                    let b = parse_label(b).map_err(ComponentParseError::InvalidExportName)?;
                    Ok(Some(PlainName::AsyncStatic(a, b)))
                }
                _ => Err(ComponentParseError::InvalidExportName(format!(
                    "Invalid async static export name: {}",
                    text
                ))),
            }
        }
        _ if is_general_label(text) => Ok(Some(PlainName::Plain(Label::new(text)))),
        _ => Ok(None),
    }
}

pub(super) fn parse_label(text: &str) -> Result<Label, String> {
    if is_general_label(text) {
        Ok(Label::new(text.to_string()))
    } else {
        Err(format!("Invalid label: {}", text))
    }
}

fn is_general_label(text: &str) -> bool {
    let mut segments = text.split('-');
    let Some(first) = segments.next() else {
        return false;
    };
    is_general_label_segment(first, false)
        && segments.all(|segment| is_general_label_segment(segment, true))
}

fn is_general_label_segment(segment: &str, allow_leading_digit: bool) -> bool {
    if segment.is_empty() {
        return false;
    }
    let mut chars = segment.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || (allow_leading_digit && first.is_ascii_digit())) {
        return false;
    }
    chars.all(|ch| ch.is_ascii_alphanumeric())
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
