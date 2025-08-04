use semver::Version;
use std::fmt::{Debug, Display, Formatter};
use std::hash::{Hash, Hasher};

pub trait StrongUnique<T> {
    fn weak_eq(&self, other: &T) -> bool;
}

#[derive(Debug, Clone)]
pub struct ExportName {
    pub parsed: ParsedExportName,
    pub original: String,
}

impl ExportName {
    pub fn new(original: impl Into<String>, parsed: ParsedExportName) -> Self {
        Self {
            original: original.into(),
            parsed,
        }
    }
}

impl PartialEq for ExportName {
    fn eq(&self, other: &Self) -> bool {
        self.original == other.original
    }
}

impl Eq for ExportName {}

impl Hash for ExportName {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.original.hash(state)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ParsedExportName {
    Plain(PlainName),
    Interface(InterfaceName),
}

impl StrongUnique<Self> for ExportName {
    fn weak_eq(&self, other: &Self) -> bool {
        self.parsed.weak_eq(&other.parsed)
    }
}

impl StrongUnique<Self> for ParsedExportName {
    fn weak_eq(&self, other: &Self) -> bool {
        match self {
            ParsedExportName::Plain(name) => match other {
                ParsedExportName::Plain(o) => name.weak_eq(o),
                _ => false,
            },
            ParsedExportName::Interface(inter) => match other {
                ParsedExportName::Plain(_) => false,
                ParsedExportName::Interface(other) => inter.flat() == other.flat(),
            },
        }
    }
}

impl ParsedExportName {
    pub fn is_plain(&self) -> bool {
        matches!(self, ParsedExportName::Plain(_))
    }

    pub fn is_plain_annotated(&self) -> bool {
        if let ParsedExportName::Plain(name) = self {
            !matches!(name, PlainName::Plain(_))
        } else {
            false
        }
    }
}

#[derive(Debug, Clone)]
pub struct ImportName {
    pub original: String,
    pub parsed: ParsedImportName,
}

impl ImportName {
    pub fn new(original: impl Into<String>, parsed: ParsedImportName) -> Self {
        Self {
            original: original.into(),
            parsed,
        }
    }
}

impl StrongUnique<Self> for ImportName {
    fn weak_eq(&self, other: &Self) -> bool {
        self.parsed.weak_eq(&other.parsed)
    }
}

impl PartialEq for ImportName {
    fn eq(&self, other: &Self) -> bool {
        self.original == other.original
    }
}

impl Eq for ImportName {}

impl Hash for ImportName {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.original.hash(state)
    }
}

#[derive(Debug, Clone)]
pub enum ParsedImportName {
    Plain(PlainName),
    Interface(InterfaceName),
    Dependency(Dependency),
    Url(UrlName),
    Hash(HashName),
}

impl StrongUnique<Self> for ParsedImportName {
    fn weak_eq(&self, other: &Self) -> bool {
        match (self, other) {
            (ParsedImportName::Plain(name), ParsedImportName::Plain(other)) => name.weak_eq(other),
            (ParsedImportName::Interface(lhs), ParsedImportName::Interface(rhs)) => {
                lhs.flat() == rhs.flat()
            }
            (ParsedImportName::Dependency(lhs), ParsedImportName::Dependency(rhs)) => {
                lhs.weak_eq(rhs)
            }
            (ParsedImportName::Url(lhs), ParsedImportName::Url(rhs)) => lhs.weak_eq(rhs),
            (ParsedImportName::Hash(lhs), ParsedImportName::Hash(rhs)) => lhs.weak_eq(rhs),
            _ => false,
        }
    }
}

impl Display for ExportName {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.original, f)
    }
}

impl Display for ImportName {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.original, f)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Label(pub String);

impl Label {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn flat(&self) -> String {
        self.0.to_ascii_lowercase()
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

impl PlainName {
    fn flat(&self) -> String {
        match self {
            PlainName::Plain(label) => label.0.to_ascii_lowercase(),
            PlainName::Constructor(label) => label.0.to_ascii_lowercase(),
            PlainName::Method(x, y) => {
                format!("{}.{}", x.0.to_ascii_lowercase(), y.0.to_ascii_lowercase())
            }
            PlainName::Static(x, y) => {
                format!("{}.{}", x.0.to_ascii_lowercase(), y.0.to_ascii_lowercase())
            }
        }
    }
}

impl StrongUnique<Self> for PlainName {
    /// 二つのPlainNameが「強く一意」の判定上で等しいかを判定します．
    fn weak_eq(&self, other: &Self) -> bool {
        match self {
            PlainName::Plain(plain) => match other {
                PlainName::Plain(_) => self.flat() == other.flat(),
                PlainName::Constructor(_) => false,
                // If one name is l and the other name is [*]l.l (for the same label l and any annotation * with a dotted l.l name), they are not strongly-unique.
                PlainName::Method(x, y) => plain.flat() == x.flat() && plain.flat() == y.flat(),
                PlainName::Static(x, y) => plain.flat() == x.flat() && plain.flat() == y.flat(),
            },
            PlainName::Constructor(x) => match other {
                PlainName::Plain(_) => false,
                PlainName::Constructor(y) => x.flat() == y.flat(),
                PlainName::Method(_, _) => false,
                PlainName::Static(_, _) => false,
            },
            PlainName::Method(lhs, rhs) | PlainName::Static(lhs, rhs) => match other {
                PlainName::Plain(p) => p.flat() == lhs.flat() && p.flat() == rhs.flat(),
                PlainName::Constructor(_) => false,
                PlainName::Method(x, y) => lhs.flat() == x.flat() && y.flat() == rhs.flat(),
                PlainName::Static(x, y) => lhs.flat() == x.flat() && y.flat() == rhs.flat(),
            },
        }
    }
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

impl InterfaceName {
    fn flat(&self) -> String {
        format!(
            "{}:{}/{}{}",
            self.namespace,
            self.label,
            self.projection,
            if let Some(version) = &self.version {
                format!("@{}", version)
            } else {
                "".into()
            }
        )
    }
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

impl StrongUnique<Self> for Dependency {
    fn weak_eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Dependency::Unlocked(lhs), Dependency::Unlocked(rhs)) => lhs.weak_eq(rhs),
            (Dependency::Locked(lhs), Dependency::Locked(rhs)) => lhs.weak_eq(rhs),
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UnlockedDependency {
    pub package: PackagePath,
    pub version_range: Option<VersionRange>,
}

impl StrongUnique<Self> for UnlockedDependency {
    fn weak_eq(&self, other: &Self) -> bool {
        self.package == other.package && self.version_range == other.version_range
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LockedDependency {
    pub package: PackagePath,
    pub version: Option<Version>,
    pub hash_name: Option<HashName>,
}

impl StrongUnique<Self> for LockedDependency {
    fn weak_eq(&self, other: &Self) -> bool {
        self.package == other.package
            && self.version == other.version
            && self.hash_name == other.hash_name
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PackagePath {
    pub namespace: String,
    pub name: String,
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UrlName {
    pub url: String,
    pub hash_name: Option<HashName>,
}

impl StrongUnique<Self> for UrlName {
    fn weak_eq(&self, other: &Self) -> bool {
        self.url == other.url && self.hash_name == other.hash_name
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HashName {
    pub integrity: String,
}

impl StrongUnique<Self> for HashName {
    fn weak_eq(&self, other: &Self) -> bool {
        self.integrity == other.integrity
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(label: &str) -> PlainName {
        PlainName::Plain(Label::new(label))
    }

    fn constructor(label: &str) -> PlainName {
        PlainName::Constructor(Label::new(label))
    }

    fn method(label: &str, method: &str) -> PlainName {
        PlainName::Method(Label::new(label), Label::new(method))
    }

    fn static_(label: &str, method: &str) -> PlainName {
        PlainName::Static(Label::new(label), Label::new(method))
    }

    #[test]
    fn test_plain_name_strong_eq() {
        assert!(plain("foo").weak_eq(&plain("foo")));
        assert!(!plain("foo").weak_eq(&plain("bar")));
        assert!(constructor("foo").weak_eq(&constructor("foo")));
        assert!(!constructor("foo").weak_eq(&constructor("bar")));
        assert!(method("foo", "bar").weak_eq(&method("foo", "bar")));
        assert!(!method("foo", "bar").weak_eq(&method("foo", "baz")));
        assert!(!method("foo", "bar").weak_eq(&method("baz", "bar")));
        assert!(static_("foo", "bar").weak_eq(&static_("foo", "bar")));
        assert!(!static_("foo", "bar").weak_eq(&static_("foo", "baz")));

        assert!(!plain("foo").weak_eq(&constructor("foo")));
        assert!(!plain("foo").weak_eq(&method("foo", "bar")));
        assert!(!plain("foo").weak_eq(&static_("foo", "bar")));

        assert!(!constructor("foo").weak_eq(&plain("foo")));
        assert!(plain("foo").weak_eq(&method("foo", "foo")));
        assert!(plain("foo").weak_eq(&static_("foo", "foo")));

        assert!(!method("foo", "bar").weak_eq(&plain("foo")));
        assert!(!method("foo", "bar").weak_eq(&constructor("foo")));
        assert!(method("foo", "bar").weak_eq(&static_("foo", "bar")));
    }
}
