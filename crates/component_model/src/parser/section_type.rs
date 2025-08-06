/// `ComponentSection` represents the different types of sections in a component.
///
/// Each variant is associated with a specific `u8` value that identifies the section type.
#[repr(u8)]
#[derive(Debug, PartialEq, Eq)]
pub enum ComponentSection {
    /// Custom section type.
    Custom = 0x00,
    /// Core module section type.
    CoreModule = 0x01,
    /// Core instance section type.
    CoreInstance = 0x02,
    /// Core type section type.
    CoreType = 0x03,
    /// Component section type.
    Component = 0x04,
    /// Instance section type.
    Instance = 0x05,
    /// Alias section type.
    Alias = 0x06,
    /// Type section type.
    Type = 0x07,
    /// Canon section type.
    Canon = 0x08,
    /// Start section type.
    Start = 0x09,
    /// RawImport section type.
    Import = 0x0a,
    /// Export section type.
    Export = 0x0b,
    /// Value section type.
    #[cfg(feature = "value-imports-exports")]
    Value = 0x0c,
}
