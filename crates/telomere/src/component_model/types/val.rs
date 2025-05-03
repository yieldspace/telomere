use crate::component_model::{Case, Label, LabelValType, PrimValType, Type};

#[derive(Debug, Clone, PartialEq)]
pub enum ValType {
    Type(Box<DefValType>),
    Primitive(PrimValType),
}

#[derive(Debug, Clone, PartialEq)]
pub enum DefValType {
    Primitive(PrimValType),
    Record(Vec<LabelValType>),
    Variant(Vec<Case>),
    List(ValType, Option<usize>),
    Tuple(Vec<ValType>),
    Flags(Vec<Label>),
    Enum(Vec<Label>),
    Option(ValType),
    Result(Option<ValType>, Option<ValType>),
    Own(Type),
    Borrow(Type),
    #[cfg(feature = "component-gated-feature-async")]
    Stream(Option<ValType>),
    #[cfg(feature = "component-gated-feature-async")]
    Future(Option<ValType>),
}
