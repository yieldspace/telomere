use crate::ir::{InterfaceName, Label};
use semver::Version;

pub(super) fn parse_interface_name_string(text: &str) -> Result<Option<InterfaceName>, String> {
    if !text.contains(':') && !text.contains('/') {
        return Ok(None);
    }
    let (path, version) = match text.split_once('@') {
        Some((_path, "")) => return Err("empty string".to_owned()),
        Some((path, version)) => (path, Some(parse_semver(version)?)),
        None => (text, None),
    };
    let Some((head, projection)) = path.split_once('/') else {
        return Err("expected `/` after package name".to_owned());
    };
    let Some(split) = head.rfind(':') else {
        return Ok(None);
    };
    let namespace = parse_words_path(&head[..split], ':')?;
    let label = parse_words(&head[split + 1..])?;
    let projection = parse_words_path(projection, '/')?;
    Ok(Some(InterfaceName {
        namespace,
        label: Label::new(label),
        version,
        projection,
    }))
}

fn parse_words(text: &str) -> Result<String, String> {
    if is_lowercase_kebab(text) {
        Ok(text.to_string())
    } else {
        Err(format!("Invalid words: {}", text))
    }
}

pub(super) fn parse_words_path(text: &str, separator: char) -> Result<String, String> {
    if text.is_empty() {
        return Err("empty string".to_owned());
    }
    for segment in text.split(separator) {
        parse_words(segment)?;
    }
    Ok(text.to_owned())
}

fn is_lowercase_kebab(text: &str) -> bool {
    let mut segments = text.split('-');
    let Some(first) = segments.next() else {
        return false;
    };
    is_lowercase_kebab_segment(first, false)
        && segments.all(|segment| is_lowercase_kebab_segment(segment, true))
}

fn is_lowercase_kebab_segment(segment: &str, allow_numeric_only: bool) -> bool {
    if segment.is_empty() || !segment.chars().all(|ch| ch.is_ascii_alphanumeric()) {
        return false;
    }
    let has_lower = segment.chars().any(|ch| ch.is_ascii_lowercase());
    let has_upper = segment.chars().any(|ch| ch.is_ascii_uppercase());
    match (has_lower, has_upper) {
        (true, false) => true,
        (false, false) => allow_numeric_only,
        _ => false,
    }
}

fn parse_semver(text: &str) -> Result<Version, String> {
    Version::parse(text).map_err(|_| format!("`{text}` is not a valid semver"))
}
