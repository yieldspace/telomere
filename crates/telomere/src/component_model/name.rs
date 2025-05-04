use semver::Version;
use std::fmt::{Debug, Display, Formatter};
use std::hash::{Hash, Hasher};

pub trait StrongUnique<T> {
    fn strong_eq(&self, other: &T) -> bool;
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
    fn strong_eq(&self, other: &Self) -> bool {
        self.parsed.strong_eq(&other.parsed)
    }
}

impl StrongUnique<Self> for ParsedExportName {
    fn strong_eq(&self, other: &Self) -> bool {
        match self {
            ParsedExportName::Plain(name) => match other {
                ParsedExportName::Plain(o) => name.strong_eq(o),
                _ => false,
            },
            ParsedExportName::Interface(inter) => match other {
                ParsedExportName::Plain(_) => false,
                ParsedExportName::Interface(other) => inter.flat() == other.flat(),
            },
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
    fn strong_eq(&self, other: &Self) -> bool {
        self.parsed.strong_eq(&other.parsed)
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
    fn strong_eq(&self, other: &Self) -> bool {
        match (self, other) {
            (ParsedImportName::Plain(name), ParsedImportName::Plain(other)) => {
                name.strong_eq(other)
            }
            (ParsedImportName::Interface(lhs), ParsedImportName::Interface(rhs)) => {
                lhs.flat() == rhs.flat()
            }
            (ParsedImportName::Dependency(lhs), ParsedImportName::Dependency(rhs)) => {
                lhs.strong_eq(rhs)
            }
            (ParsedImportName::Url(lhs), ParsedImportName::Url(rhs)) => lhs.strong_eq(rhs),
            (ParsedImportName::Hash(lhs), ParsedImportName::Hash(rhs)) => lhs.strong_eq(rhs),
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
    /// 二つのPlainNameが「強く独立」の判定上で等しいかを判定します．
    fn strong_eq(&self, other: &Self) -> bool {
        match self {
            PlainName::Plain(plain) => match other {
                PlainName::Plain(_) => self.flat() == other.flat(),
                PlainName::Constructor(_) => false,
                // If one name is l and the other name is [*]l.l (for the same label l and any annotation * with a dotted l.l name), they are not strongly-unique.
                PlainName::Method(x, y) => plain.flat() == x.flat() && y.flat() == other.flat(),
                PlainName::Static(x, y) => plain.flat() == x.flat() && y.flat() == other.flat(),
            },
            PlainName::Constructor(_) => match other {
                PlainName::Plain(_) => false,
                PlainName::Constructor(_) => self.flat() == other.flat(),
                PlainName::Method(_, _) => false,
                PlainName::Static(_, _) => false,
            },
            PlainName::Method(lhs, rhs) | PlainName::Static(lhs, rhs) => match other {
                PlainName::Plain(p) => p.flat() == lhs.flat() && p.flat() == rhs.flat(),
                PlainName::Constructor(_) => false,
                PlainName::Method(_, _) => self.flat() == other.flat(),
                PlainName::Static(_, _) => self.flat() == other.flat(),
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
    fn strong_eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Dependency::Unlocked(lhs), Dependency::Unlocked(rhs)) => lhs.strong_eq(rhs),
            (Dependency::Locked(lhs), Dependency::Locked(rhs)) => lhs.strong_eq(rhs),
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
    fn strong_eq(&self, other: &Self) -> bool {
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
    fn strong_eq(&self, other: &Self) -> bool {
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
    fn strong_eq(&self, other: &Self) -> bool {
        self.url == other.url && self.hash_name == other.hash_name
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HashName {
    pub integrity: String,
}

impl StrongUnique<Self> for HashName {
    fn strong_eq(&self, other: &Self) -> bool {
        self.integrity == other.integrity
    }
}
