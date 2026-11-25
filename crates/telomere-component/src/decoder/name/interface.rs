use crate::ir::{InterfaceName, Label};
use semver::Version;

/// The structural parts of an interface name, with the version kept as
/// the raw text after `@`.
///
/// The version prefix is deliberately not validated here: whether it is
/// a valid version depends on the `version-suffix` name option that may
/// accompany the name in the split extern-name encoding (prefix 0x02),
/// so resolution happens in [`resolve_interface_version`].
pub(super) struct InterfaceNameParts {
    pub namespace: String,
    pub label: Label,
    pub projection: String,
    pub version_prefix: Option<String>,
}

/// Parses the package-path grammar of an interface name:
/// `<namespace>:<package>/<projection>[@<version-prefix>]`, where the
/// namespace, package and projection are each a single kebab segment.
/// Nested namespaces and nested projections are gated future syntax and
/// are rejected. Returns `Ok(None)` when `text` has no package-path
/// shape at all (no `:` before the first `/`).
pub(super) fn parse_interface_name_parts(text: &str) -> Result<Option<InterfaceNameParts>, String> {
    if !text.contains(':') && !text.contains('/') {
        return Ok(None);
    }
    let (path, version_prefix) = match text.split_once('@') {
        Some((_path, "")) => return Err("empty string".to_owned()),
        Some((path, version)) => (path, Some(version.to_owned())),
        None => (text, None),
    };
    let Some((head, projection)) = path.split_once('/') else {
        return Err("expected `/` after package name".to_owned());
    };
    let Some((namespace, label)) = head.split_once(':') else {
        return Ok(None);
    };
    if label.contains(':') {
        // A second `:` in the head means another namespace segment
        // follows the package name where the grammar requires `/`.
        return Err("expected `/` after package name".to_owned());
    }
    let namespace = parse_words(namespace)?;
    let label = parse_words(label)?;
    if let Some((_segment, rest)) = projection.split_once('/') {
        return Err(format!("trailing characters found: `/{rest}`"));
    }
    let projection = parse_words(projection)?;
    Ok(Some(InterfaceNameParts {
        namespace,
        label: Label::new(label),
        projection,
        version_prefix,
    }))
}

/// Resolves the full version of an interface name from its written
/// version prefix and an optional `version-suffix` name option.
///
/// Without a suffix the prefix must itself be a valid semver. With a
/// suffix the concatenation must be a valid semver and the split point
/// must be the canonical one (see [`split_canonical_version`]).
pub(super) fn resolve_interface_version(
    prefix: Option<&str>,
    suffix: Option<&str>,
) -> Result<Option<Version>, String> {
    match (prefix, suffix) {
        (None, None) => Ok(None),
        (None, Some(_)) => Err("a version suffix requires the name to have a version".to_owned()),
        (Some(prefix), None) => parse_semver(prefix).map(Some),
        (Some(prefix), Some(suffix)) => {
            let full = format!("{prefix}{suffix}");
            let version = parse_semver(&full)?;
            if split_canonical_version(&full) != Some((prefix, Some(suffix))) {
                return Err(format!(
                    "`{prefix}` is not the canonical version of `{full}`"
                ));
            }
            Ok(Some(version))
        }
    }
}

/// Builds an [`InterfaceName`] from its structural parts, resolving the
/// version against the optional `version-suffix`.
pub(super) fn build_interface_name(
    parts: InterfaceNameParts,
    version_suffix: Option<&str>,
) -> Result<InterfaceName, String> {
    let version = resolve_interface_version(parts.version_prefix.as_deref(), version_suffix)?;
    Ok(InterfaceName {
        namespace: parts.namespace,
        label: parts.label,
        projection: parts.projection,
        version,
    })
}

#[cfg(test)]
pub(super) fn parse_interface_name_string(text: &str) -> Result<Option<InterfaceName>, String> {
    match parse_interface_name_parts(text)? {
        Some(parts) => Ok(Some(build_interface_name(parts, None)?)),
        None => Ok(None),
    }
}

/// Splits a full version into its canonical prefix and the remaining
/// suffix, or returns `None` when `version` is not a valid semver.
///
/// The canonical split point is after the major version, or after
/// `0.minor` for `0.x` versions, when there is no pre-release; a version
/// with a pre-release is not split at all (only its build metadata can
/// be a suffix). Ported from wasmparser's `split_canonical_version`.
pub(super) fn split_canonical_version(version: &str) -> Option<(&str, Option<&str>)> {
    let is_identifier =
        |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
    let (without_build, build) = match version.split_once('+') {
        Some((a, b)) => (a, Some(b)),
        None => (version, None),
    };
    if build.is_some_and(|b| !b.split('.').all(is_identifier)) {
        return None;
    }
    let (core, pre) = match without_build.split_once('-') {
        Some((a, b)) => (a, Some(b)),
        None => (without_build, None),
    };
    if pre.is_some_and(|p| !p.split('.').all(is_identifier)) {
        return None;
    }
    let mut numbers = core.split('.');
    let (Some(major), Some(minor), Some(patch), None) = (
        numbers.next(),
        numbers.next(),
        numbers.next(),
        numbers.next(),
    ) else {
        return None;
    };
    if ![major, minor, patch].into_iter().all(is_version_number) {
        return None;
    }

    let split = if pre.is_none() && major != "0" {
        major.len()
    } else if pre.is_none() && minor != "0" {
        major.len() + 1 + minor.len()
    } else {
        without_build.len()
    };
    match version.split_at(split) {
        (canonical, "") => Some((canonical, None)),
        (canonical, suffix) => Some((canonical, Some(suffix))),
    }
}

fn is_version_number(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && (s == "0" || !s.starts_with('0'))
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
