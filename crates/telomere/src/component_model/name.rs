use semver::Version;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ExportName {
    Plain(PlainName),
    Interface(InterfaceName),
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
        self.0.fmt(f)
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InterfaceName {
    pub namespace: String,
    pub label: Label,
    pub projection: Label,
    pub version: Option<Version>,
}
