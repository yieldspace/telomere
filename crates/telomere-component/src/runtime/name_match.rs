//! Semver-compatible matching for component names.
//!
//! Component names may carry a semver version, for example
//! `wasi:cli/run@0.2.0`. Following the name-resolution rules used by the
//! Component Model ecosystem (as implemented by Wasmtime's `NameMap`), a
//! lookup for one version may resolve to a definition at another version on
//! the same "compatible track":
//!
//! - exact matches always win (handled by the caller before consulting this
//!   module);
//! - for `major >= 1`, every version with the same major is on one track;
//! - for `0.x` with `minor >= 1`, every patch release within that minor is on
//!   one track, so a definition registered as `wasi:cli/run@0.2.0`
//!   satisfies a lookup for `wasi:cli/run@0.2.6`, and vice versa;
//! - `0.0.x` versions and prerelease versions have no compatible track and
//!   only ever match exactly.
//!
//! When several definitions share a track, the maximal version wins, again
//! mirroring Wasmtime. Matching is symmetric within a track because an
//! interface published at two patch versions is required to be the same API.

use semver::Version;

/// Returns the compatible-track key for `name` together with its parsed
/// version, or `None` when the name carries no usable semver track.
///
/// The key is the name truncated at the end of its track prefix, for example
/// `a:b/c@1` for `a:b/c@1.1.2` and `a:b/c@0.2` for `a:b/c@0.2.6`.
pub(crate) fn compatible_track(name: &str) -> Option<(String, Version)> {
    let at = name.find('@')?;
    let version_string = &name[at + 1..];
    let version = Version::parse(version_string).ok()?;
    if !version.pre.is_empty() {
        // A prerelease is not considered compatible with any other version.
        return None;
    }
    if version.major != 0 {
        // Compatibility is bounded by the major version, so the track key
        // ends just before the first dot of the version.
        let first_dot = version_string.find('.')? + at + 1;
        Some((name[..first_dot].to_owned(), version))
    } else if version.minor != 0 {
        // For 0.x, patch releases within one minor form a compatible track,
        // so the track key ends just before the second dot of the version.
        let first_dot = version_string.find('.')? + at + 1;
        let second_dot = name[first_dot + 1..].find('.')? + first_dot + 1;
        Some((name[..second_dot].to_owned(), version))
    } else {
        // 0.0.x: the patch is the first nonzero component, so nothing else is
        // inherently compatible with it.
        None
    }
}

/// Finds the name in `available` that is semver-compatible with `requested`.
///
/// Used both for a component's own export names and for the names a
/// linker registration is stored under, so import resolution and export
/// resolution follow exactly the same rules.
///
/// `available` must not contain `requested` itself (exact matches are the
/// caller's responsibility). When several names share the requested track,
/// the one with the maximal version is returned.
pub(crate) fn compatible_name<'a>(
    requested: &str,
    available: impl Iterator<Item = &'a str>,
) -> Option<&'a str> {
    let (track, _) = compatible_track(requested)?;
    let mut best: Option<(&str, Version)> = None;
    for candidate in available {
        let Some((candidate_track, candidate_version)) = compatible_track(candidate) else {
            continue;
        };
        if candidate_track != track {
            continue;
        }
        if best
            .as_ref()
            .is_none_or(|(_, best_version)| candidate_version > *best_version)
        {
            best = Some((candidate, candidate_version));
        }
    }
    best.map(|(name, _)| name)
}

/// Finds a name in `available` for the same unversioned interface as
/// `requested` but on an incompatible track, for use in error messages that
/// name both what was expected and what was found.
///
/// Returns the maximal-version such name, or `None` when no available
/// name shares the requested name's unversioned prefix.
pub(crate) fn incompatible_name<'a>(
    requested: &str,
    available: impl Iterator<Item = &'a str>,
) -> Option<&'a str> {
    let base = requested.split_once('@')?.0;
    let mut best: Option<(&str, Version)> = None;
    for candidate in available {
        if candidate == requested {
            continue;
        }
        let Some((candidate_base, version_string)) = candidate.split_once('@') else {
            continue;
        };
        if candidate_base != base {
            continue;
        }
        let Ok(version) = Version::parse(version_string) else {
            continue;
        };
        if best
            .as_ref()
            .is_none_or(|(_, best_version)| version > *best_version)
        {
            best = Some((candidate, version));
        }
    }
    best.map(|(name, _)| name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(name: &str) -> Option<String> {
        compatible_track(name).map(|(key, _)| key)
    }

    #[test]
    fn track_keys_follow_ecosystem_rules() {
        assert_eq!(track("x"), None);
        assert_eq!(track("x:y/z"), None);
        assert_eq!(track("x:y/z@1.0.0").as_deref(), Some("x:y/z@1"));
        assert_eq!(track("x:y/z@1.1.2").as_deref(), Some("x:y/z@1"));
        assert_eq!(track("x:y/z@2.1.2+abc").as_deref(), Some("x:y/z@2"));
        assert_eq!(track("x:y/z@0.1.2").as_deref(), Some("x:y/z@0.1"));
        assert_eq!(track("x:y/z@0.2.3+abc").as_deref(), Some("x:y/z@0.2"));
        assert_eq!(track("x:y/z@0.0.1"), None);
        assert_eq!(track("x:y/z@0.0.1-pre"), None);
        assert_eq!(track("x:y/z@0.1.0-pre"), None);
        assert_eq!(track("x:y/z@1.0.0-pre"), None);
        assert_eq!(track("x:y/z@not-a-version"), None);
    }

    #[test]
    fn compatible_lookup_matches_within_zero_minor_track() {
        // The #117 case: the guest exports 0.2.0, the host asks for 0.2.6.
        let available = ["wasi:cli/run@0.2.0"];
        assert_eq!(
            compatible_name("wasi:cli/run@0.2.6", available.iter().copied()),
            Some("wasi:cli/run@0.2.0")
        );
        // And the reverse direction resolves too.
        let available = ["wasi:cli/run@0.2.6"];
        assert_eq!(
            compatible_name("wasi:cli/run@0.2.0", available.iter().copied()),
            Some("wasi:cli/run@0.2.6")
        );
    }

    #[test]
    fn compatible_lookup_prefers_maximal_version_on_track() {
        let available = ["a:b/c@1.0.0", "a:b/c@1.0.1", "a:b/c@0.9.9"];
        assert_eq!(
            compatible_name("a:b/c@1.0.2", available.iter().copied()),
            Some("a:b/c@1.0.1")
        );
        assert_eq!(
            compatible_name("a:b/c@1.1.0", available.iter().copied()),
            Some("a:b/c@1.0.1")
        );
    }

    #[test]
    fn compatible_lookup_rejects_other_tracks() {
        // Different minor within 0.x.
        let available = ["wasi:cli/run@0.3.0"];
        assert_eq!(
            compatible_name("wasi:cli/run@0.2.6", available.iter().copied()),
            None
        );
        // Different major.
        let available = ["a:b/c@2.0.0"];
        assert_eq!(
            compatible_name("a:b/c@1.0.0", available.iter().copied()),
            None
        );
        // 0.0.x has no track.
        let available = ["a:b/c@0.0.2"];
        assert_eq!(
            compatible_name("a:b/c@0.0.1", available.iter().copied()),
            None
        );
        // Prereleases never widen.
        let available = ["a:b/c@0.2.0"];
        assert_eq!(
            compatible_name("a:b/c@0.2.1-rc.1", available.iter().copied()),
            None
        );
        // Unversioned names and different interfaces never match.
        let available = ["wasi:cli/run", "wasi:cli/other@0.2.0"];
        assert_eq!(
            compatible_name("wasi:cli/run@0.2.6", available.iter().copied()),
            None
        );
    }

    #[test]
    fn incompatible_lookup_names_the_found_version() {
        let available = [
            "wasi:cli/run@0.3.0",
            "wasi:cli/run@0.1.0",
            "other:pkg/x@9.9.9",
        ];
        assert_eq!(
            incompatible_name("wasi:cli/run@0.2.6", available.iter().copied()),
            Some("wasi:cli/run@0.3.0")
        );
        let available = ["wasi:cli/other@0.3.0"];
        assert_eq!(
            incompatible_name("wasi:cli/run@0.2.6", available.iter().copied()),
            None
        );
        // Unversioned requests have nothing to report.
        let available = ["wasi:cli/run@0.3.0"];
        assert_eq!(
            incompatible_name("wasi:cli/run", available.iter().copied()),
            None
        );
    }
}
