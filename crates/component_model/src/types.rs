mod externdesc;
mod primitive;
mod validator;

pub use externdesc::*;
pub use validator::*;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct TypeIdx(u32);

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct CoreTypeId(u32);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TypeDef {
    Interface(InterfaceType),
    Func,
    Component,
    Instance,
    Resource,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum InterfaceType {}
