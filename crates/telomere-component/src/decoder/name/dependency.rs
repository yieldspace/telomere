use crate::ir::{
    Dependency, HashName, LockedDependency, PackagePath, UnlockedDependency, UrlName, VersionRange,
};
use semver::Version;

pub(super) fn parse_dep_name_string(text: &str) -> Result<Option<Dependency>, String> {
    if let Some(rest) = text.strip_prefix("unlocked-dep=") {
        return Ok(Some(Dependency::Unlocked(parse_unlocked_dependency(rest)?)));
    }
    if let Some(rest) = text.strip_prefix("locked-dep=") {
        return Ok(Some(Dependency::Locked(parse_locked_dependency(rest)?)));
    }
    Ok(None)
}

pub(super) fn parse_url_name_string(text: &str) -> Result<Option<UrlName>, String> {
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

pub(super) fn parse_hash_name_string(text: &str) -> Result<Option<HashName>, String> {
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

pub(super) fn parse_package_path(rest: &str) -> Result<(PackagePath, &str), String> {
    let end = rest.find(['@', '>']).unwrap_or(rest.len());
    let path = &rest[..end];
    let trailing = &rest[end..];
    let Some(split) = path.rfind(':') else {
        return Err("expected `/` after package name".to_owned());
    };
    let namespace = super::interface::parse_words_path(&path[..split], ':')?;
    let name = super::interface::parse_words_path(&path[split + 1..], '/')?;
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
