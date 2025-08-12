use crate::types::{PrimValType, TypeId};

#[derive(Debug, Hash, Eq, PartialEq, Clone)]
pub enum ValType {
    Primitive(PrimValType),
    Record(Vec<TypeId>),
    Variant(Vec<TypeId>),
    List(TypeId),
    ListWithSize(TypeId, u32),
    Tuple(Vec<TypeId>),
    Flags(Vec<TypeId>),
    Option(TypeId),
    Result(Option<TypeId>, Option<TypeId>),
    Own(TypeId),
    Borrow(TypeId),
    #[cfg(feature = "async")]
    Stream(Option<TypeId>),
    #[cfg(feature = "async")]
    Future(Option<TypeId>),
}
