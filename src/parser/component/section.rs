#[repr(u8)]
#[derive(Debug, PartialEq, Eq)]
pub enum ComponentSectionType {
    Custom = 0x00,
    CoreModule = 0x01,
    CoreInstance = 0x02,
    CoreType = 0x03,
    Component = 0x04,
    Instance = 0x05,
    Alias = 0x06,
    Type = 0x07,
    Canon = 0x08,
    Start = 0x09,
    Import = 0x0a,
    Export = 0x0b,
    Value = 0x0c,
}
