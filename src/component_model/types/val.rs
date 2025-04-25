use crate::component_model::{Case, Label, LabelValType, PrimValType, TypeIdx};

#[derive(Debug, Clone)]
pub enum ValType {
    TypeId(TypeIdx),
    Primitive(PrimValType),
}

#[derive(Debug, Clone)]
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
    Own(TypeIdx),
    Borrow(TypeIdx),
    #[cfg(feature = "component-gated-feature-async")]
    Stream(Option<ValType>),
    #[cfg(feature = "component-gated-feature-async")]
    Future(Option<ValType>),
}
