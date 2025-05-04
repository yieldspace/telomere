use semver::Version;
use std::fmt::{Debug, Display, Formatter};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ExportName {
    Plain(PlainName),
    Interface(InterfaceName),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ImportName {
    Plain(PlainName),
    Interface(InterfaceName),
    Dependency(Dependency),
    Url(UrlName),
    Hash(HashName),
}

impl Display for ExportName {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ExportName::Plain(s) => <PlainName as Display>::fmt(s, f),
            ExportName::Interface(s) => <InterfaceName as Display>::fmt(s, f),
        }
    }
}

impl Display for ImportName {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ImportName::Plain(s) => <PlainName as Display>::fmt(s, f),
            ImportName::Interface(s) => <InterfaceName as Display>::fmt(s, f),
            ImportName::Dependency(s) => <Dependency as Display>::fmt(s, f),
            ImportName::Url(s) => <UrlName as Display>::fmt(s, f),
            ImportName::Hash(s) => <HashName as Display>::fmt(s, f),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Label(pub String);

impl Label {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

impl Display for Label {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.0, f)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PlainName {
    /// <label>
    Plain(Label),
    #[cfg(feature = "component-gated-feature-async")]
    /// [async] <label>
    Async(Label, Label),
    /// [constructor] <label>
    Constructor(Label),
    /// [method] <label> . <label>
    Method(Label, Label),
    #[cfg(feature = "component-gated-feature-async")]
    /// [async method] <label> . <label>
    AsyncMethod(Label, Label),
    /// [static] <string> . <string>
    Static(Label, Label),
    #[cfg(feature = "component-gated-feature-async")]
    /// [async static] <string> . <string>
    AsyncStatic(Label, Label),
}

impl Display for PlainName {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            PlainName::Plain(l) => write!(f, "{}", l),
            PlainName::Constructor(l) => write!(f, "[constructor]{}", l),
            PlainName::Method(l, r) => write!(f, "[method]{}.{}", l, r),
            PlainName::Static(l, r) => write!(f, "[static]{}.{}", l, r),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InterfaceName {
    pub namespace: String,
    pub label: Label,
    pub projection: Label,
    pub version: Option<Version>,
}

impl Display for InterfaceName {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}/{}", self.namespace, self.label, self.projection)?;
        if let Some(version) = &self.version {
            write!(f, "@{}", version)
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Dependency {
    Unlocked(UnlockedDependency),
    Locked(LockedDependency),
}

impl Display for Dependency {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Dependency::Unlocked(dep) => <UnlockedDependency as Display>::fmt(dep, f),
            Dependency::Locked(dep) => <LockedDependency as Display>::fmt(dep, f),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UnlockedDependency {
    pub package: PackagePath,
    pub version_range: Option<VersionRange>,
}

impl Display for UnlockedDependency {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "unlocked-dep=<{}", self.package)?;
        if let Some(version_range) = &self.version_range {
            write!(f, "{}", version_range)?;
        }
        write!(f, ">")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LockedDependency {
    pub package: PackagePath,
    pub version: Option<Version>,
    pub hash_name: Option<HashName>,
}

impl Display for LockedDependency {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "locked-dep=<{}", self.package)?;
        if let Some(version) = &self.version {
            write!(f, "@{}", version)?;
        }
        write!(f, ">")?;
        if let Some(hash_name) = &self.hash_name {
            write!(f, ",{}", hash_name)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PackagePath {
    pub namespace: String,
    pub name: String,
}

impl Display for PackagePath {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.namespace, self.name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum VersionRange {
    /// same as @*
    Any,
    Ranged {
        lower: Option<Version>,
        upper: Option<Version>,
    },
}

impl Display for VersionRange {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            VersionRange::Any => write!(f, "@*"),
            VersionRange::Ranged { lower, upper } => match (lower, upper) {
                (Some(lower), None) => write!(f, "@{{>={}}}", lower),
                (None, Some(upper)) => write!(f, "@{{<{}}}", upper),
                (Some(lower), Some(upper)) => write!(f, "@{{>={} <{}}}", lower, upper),
                _ => unreachable!(),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UrlName {
    pub url: String,
    pub hash_name: Option<HashName>,
}

impl Display for UrlName {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "url=<{}>", self.url)?;
        if let Some(hash_name) = &self.hash_name {
            write!(f, ",{}", hash_name)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HashName {
    pub integrity: String,
}

impl Display for HashName {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "integrity=<{}>", self.integrity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_plain_name() {
        assert_eq!(format!("{}", PlainName::Plain(Label::new("test"))), "test");
        assert_eq!(
            format!("{}", PlainName::Constructor(Label::new("test"))),
            "[constructor]test"
        );
        assert_eq!(
            format!(
                "{}",
                PlainName::Method(Label::new("test"), Label::new("test2"))
            ),
            "[method]test.test2"
        );
        assert_eq!(
            format!(
                "{}",
                PlainName::Static(Label::new("test"), Label::new("test3"))
            ),
            "[static]test.test3"
        );
    }

    #[test]
    fn test_display_interface_name() {
        let interface_name = InterfaceName {
            namespace: "test".to_string(),
            label: Label::new("test"),
            projection: Label::new("test2"),
            version: Some(Version::parse("1.0.0").unwrap()),
        };
        assert_eq!(format!("{}", interface_name), "test:test/test2@1.0.0");
    }

    #[test]
    fn test_display_dependency() {
        let unlocked_dep = UnlockedDependency {
            package: PackagePath {
                namespace: "test".to_string(),
                name: "test".to_string(),
            },
            version_range: Some(VersionRange::Ranged {
                lower: Some(Version::parse("1.0.0").unwrap()),
                upper: None,
            }),
        };
        assert_eq!(
            format!("{}", Dependency::Unlocked(unlocked_dep)),
            "unlocked-dep=<test:test@{>=1.0.0}>"
        );

        let locked_dep = LockedDependency {
            package: PackagePath {
                namespace: "test".to_string(),
                name: "test".to_string(),
            },
            version: Some(Version::parse("1.0.0").unwrap()),
            hash_name: Some(HashName {
                integrity: "sha256-abc123".to_string(),
            }),
        };
        assert_eq!(
            format!("{}", Dependency::Locked(locked_dep)),
            "locked-dep=<test:test@1.0.0>,integrity=<sha256-abc123>"
        );
    }
}
