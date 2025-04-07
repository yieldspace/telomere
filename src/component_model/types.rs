use crate::component_model::id::{FuncId, TypeId};
use crate::component_model::{Alias, CoreType};
use crate::parser::leb128::compile_i32;
use num_derive::FromPrimitive;
use std::sync::{Arc, Weak};

#[derive(Debug, Clone)]
pub enum TypeKind {
    Type(Weak<Type>),
    Alias(Weak<Alias>),
    Import,
}

#[derive(Debug)]
pub enum Type {
    DefVal(DefValType),
    Func(FuncType),
    Component(ComponentType),
    Instance(InstanceType),
    Resource(ResourceType),
}

#[derive(Debug, FromPrimitive)]
#[repr(i32)]
pub enum PrimValType {
    Bool = compile_i32([0x7f]),
    S8 = compile_i32([0x7e]),
    U8 = compile_i32([0x7d]),
    S16 = compile_i32([0x7c]),
    U16 = compile_i32([0x7b]),
    S32 = compile_i32([0x7a]),
    U32 = compile_i32([0x79]),
    S64 = compile_i32([0x78]),
    U64 = compile_i32([0x77]),
    F32 = compile_i32([0x76]),
    F64 = compile_i32([0x75]),
    Char = compile_i32([0x74]),
    String = compile_i32([0x73]),
    #[cfg(feature = "async")]
    ErrorContext = compile_i32([0x64]),
}

#[derive(Debug)]
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
    Own(TypeId),
    Borrow(TypeId),
    #[cfg(feature = "async")]
    Stream(Option<ValType>),
    #[cfg(feature = "async")]
    Future(Option<ValType>),
}

#[derive(Debug)]
pub struct LabelValType {
    pub label: Label,
    pub t: ValType,
}

#[derive(Debug)]
pub struct Case {
    pub label: Label,
    pub t: Option<ValType>,
}

#[derive(Debug)]
pub struct Label {
    pub len: usize,
    pub label: String, // TODO: check label format https://github.com/WebAssembly/component-model/blob/main/design/mvp/Explainer.md#import-and-export-definitions
}

#[derive(Debug)]
pub enum ValType {
    TypeId(TypeId),
    Primitive(PrimValType),
}

#[derive(Debug)]
pub enum ResourceType {
    Resource(Option<FuncId>),
    ResourceWithAsyncCallback(FuncId, Option<FuncId>),
}

#[derive(Debug)]
pub struct FuncType {
    pub params: Vec<LabelValType>,
    pub result: Option<ValType>,
}

#[derive(Debug)]
pub struct ComponentType(pub Vec<ComponentDecl>);

#[derive(Debug)]
pub enum ComponentDecl {
    Import(ImportDecl),
    Instance(InstanceDecl),
}

#[derive(Debug)]
pub struct InstanceType(pub Vec<InstanceDecl>);

#[derive(Debug)]
pub enum InstanceDecl {
    CoreType(CoreType),
    Type(Type),
    Alias(Alias),
    ExportDecl(ExportDecl),
}

#[derive(Debug)]
pub struct ImportDecl {
    pub name: String,
    pub ed: ExternDesc,
}

#[derive(Debug)]
pub struct ExportDecl {
    pub name: String,
    pub ed: ExternDesc,
}

#[derive(Debug)]
pub enum ExternDesc {
    Core(usize),
    Func(usize),
    #[cfg(feature = "import_export")]
    Value(ValueBound),
    Type(TypeBound),
    Component(usize),
    Instance(usize),
}

#[derive(Debug)]
pub enum TypeBound {
    Eq(TypeId),
    Sub,
}

#[derive(Debug)]
#[cfg(feature = "import_export")]
pub enum ValueBound {
    Eq(usize),
    Type(ValType),
}

#[cfg(test)]
mod tests {
    use crate::component_model::types::PrimValType;
    use crate::parser::leb128::compile_i32;

    #[test]
    fn test_prim() {
        assert_eq!(compile_i32([0x7f]), PrimValType::Bool as i32);
    }
}
