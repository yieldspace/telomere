use crate::types::{CoreTypeId, TypeIdx};

pub enum ExternDesc {
    CoreModule(CoreTypeId),
    Func(TypeIdx),
    #[cfg(feature = "value-imports-exports")]
    Value,
    Type(TypeBound),
    Component(TypeIdx),
    Instance(TypeIdx),
}

pub enum TypeBound {
    Eq(TypeIdx),
    Sub,
}
